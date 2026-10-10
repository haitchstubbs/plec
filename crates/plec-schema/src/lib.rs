pub mod delta;
pub mod js_decode;
pub mod js_encode;
pub mod json_encode;
pub mod routing;
pub mod ssr_decode;
pub mod typed;

pub use delta::RuntimeValue;
pub use plec_ir::limits;

/// Canonical conversion between host loader failures and the public route
/// failure record shared by SSR snapshots and browser navigation.
pub fn public_route_loader_failure(value: &RuntimeValue) -> plec_ir::PublicRouteLoaderFailure {
    use plec_ir::{PublicRouteLoaderFailure as Public, PublicRouteLoaderFailureKind as Kind};
    let Some(record) = value.record() else {
        return Public::generic();
    };
    let Some(RuntimeValue::String(kind)) = record.get("kind") else {
        return Public::generic();
    };
    let kind = match kind.as_str() {
        "http" => Kind::Http,
        "network" => Kind::Network,
        "abort" => Kind::Abort,
        "decode" => Kind::Decode,
        _ => return Public::generic(),
    };
    let Some(RuntimeValue::String(message)) = record.get("message") else {
        return Public::generic();
    };
    let status = match record.get("status") {
        None => None,
        Some(RuntimeValue::Number(n))
            if n.is_finite() && n.fract() == 0.0 && (100.0..=599.0).contains(n) =>
        {
            Some(*n as u16)
        }
        Some(_) => return Public::generic(),
    };
    if status.is_some() && kind != Kind::Http {
        return Public::generic();
    }
    let status_text = match record.get("statusText") {
        None => None,
        Some(RuntimeValue::String(_)) if kind == Kind::Http => match status {
            Some(status) => Some(plec_ir::canonical_status_text(status).to_owned()),
            None => return Public::generic(),
        },
        Some(RuntimeValue::String(_)) => return Public::generic(),
        Some(_) => return Public::generic(),
    };
    let body = record
        .get("body")
        .cloned()
        .map(RuntimeValue::into_ssr_snapshot);
    let url = match record.get("url") {
        None => None,
        Some(RuntimeValue::String(s)) => Some(s.clone()),
        Some(_) => return Public::generic(),
    };
    let failure = Public {
        kind,
        message: message.clone(),
        status,
        status_text,
        body,
        url,
    };
    match failure.validate() {
        Ok(()) => failure,
        Err("route loader failure body exceeds limit") => {
            let mut bounded = failure;
            bounded.body = None;
            if bounded.validate().is_ok() {
                bounded
            } else {
                Public::generic()
            }
        }
        Err(_) => Public::generic(),
    }
}

/// Restore the application-visible RuntimeValue record from the snapshot form.
pub fn route_loader_failure_value(failure: &plec_ir::PublicRouteLoaderFailure) -> RuntimeValue {
    use plec_ir::PublicRouteLoaderFailureKind as Kind;
    let kind = match failure.kind {
        Kind::Http => "http",
        Kind::Network => "network",
        Kind::Abort => "abort",
        Kind::Decode => "decode",
        Kind::Runtime => "runtime",
    };
    let mut record = std::collections::HashMap::from([
        ("kind".into(), RuntimeValue::String(kind.into())),
        (
            "message".into(),
            RuntimeValue::String(failure.message.clone()),
        ),
    ]);
    if let Some(status) = failure.status {
        record.insert("status".into(), RuntimeValue::Number(status as f64));
    }
    if let Some(text) = &failure.status_text {
        record.insert("statusText".into(), RuntimeValue::String(text.clone()));
    }
    if let Some(body) = &failure.body {
        record.insert("body".into(), RuntimeValue::from_ssr_snapshot(body));
    }
    if let Some(url) = &failure.url {
        record.insert("url".into(), RuntimeValue::String(url.clone()));
    }
    RuntimeValue::Record(record)
}

#[cfg(test)]
mod route_failure_tests {
    use super::*;
    use plec_ir::{PublicRouteLoaderFailureKind as Kind, SsrSnapshotValue};

    #[test]
    fn shared_failure_conversion_preserves_http_record_fields() {
        let source = RuntimeValue::Record(std::collections::HashMap::from([
            ("kind".into(), RuntimeValue::String("http".into())),
            (
                "message".into(),
                RuntimeValue::String("request failed (503)".into()),
            ),
            ("status".into(), RuntimeValue::Number(503.0)),
            (
                "statusText".into(),
                RuntimeValue::String("Unavailable".into()),
            ),
            ("body".into(), RuntimeValue::String("maintenance".into())),
            ("url".into(), RuntimeValue::String("/api/data".into())),
        ]));
        let public = public_route_loader_failure(&source);
        assert_eq!(public.kind, Kind::Http);
        assert_eq!(public.status, Some(503));
        assert_eq!(public.status_text.as_deref(), Some("Service Unavailable"));
        assert_eq!(public.url.as_deref(), Some("/api/data"));
        assert_eq!(
            public.body,
            Some(SsrSnapshotValue::String("maintenance".into()))
        );
        let restored = route_loader_failure_value(&public);
        assert_eq!(
            restored.record().unwrap().get("statusText"),
            Some(&RuntimeValue::String("Service Unavailable".into()))
        );
    }

    #[test]
    fn status_text_is_canonicalized_only_for_http_with_status() {
        let http_without_phrase = RuntimeValue::Record(std::collections::HashMap::from([
            ("kind".into(), RuntimeValue::String("http".into())),
            (
                "message".into(),
                RuntimeValue::String("request failed (599)".into()),
            ),
            ("status".into(), RuntimeValue::Number(599.0)),
        ]));
        let public = public_route_loader_failure(&http_without_phrase);
        assert_eq!(public.status, Some(599));
        assert_eq!(public.status_text, None);

        let http_with_unknown_phrase = RuntimeValue::Record(std::collections::HashMap::from([
            ("kind".into(), RuntimeValue::String("http".into())),
            (
                "message".into(),
                RuntimeValue::String("request failed (599)".into()),
            ),
            ("status".into(), RuntimeValue::Number(599.0)),
            (
                "statusText".into(),
                RuntimeValue::String("upstream phrase".into()),
            ),
        ]));
        let public = public_route_loader_failure(&http_with_unknown_phrase);
        assert_eq!(public.status, Some(599));
        assert_eq!(public.status_text.as_deref(), Some(""));

        let phrase_without_status = RuntimeValue::Record(std::collections::HashMap::from([
            ("kind".into(), RuntimeValue::String("http".into())),
            ("message".into(), RuntimeValue::String("failure".into())),
            ("statusText".into(), RuntimeValue::String("OK".into())),
        ]));
        assert_eq!(
            public_route_loader_failure(&phrase_without_status),
            plec_ir::PublicRouteLoaderFailure::generic()
        );
    }

    #[test]
    fn oversized_supplementary_body_is_dropped_but_http_failure_is_preserved() {
        let source = RuntimeValue::Record(std::collections::HashMap::from([
            ("kind".into(), RuntimeValue::String("http".into())),
            (
                "message".into(),
                RuntimeValue::String("request failed (500)".into()),
            ),
            ("status".into(), RuntimeValue::Number(500.0)),
            ("body".into(), RuntimeValue::String("x".repeat(600_000))),
        ]));
        let public = public_route_loader_failure(&source);
        assert_eq!(public.kind, Kind::Http);
        assert_eq!(public.status, Some(500));
        assert_eq!(public.body, None);
    }

    #[test]
    fn malformed_runtime_failure_uses_generic_public_record() {
        let source = RuntimeValue::Record(std::collections::HashMap::from([
            ("kind".into(), RuntimeValue::String("internal".into())),
            (
                "message".into(),
                RuntimeValue::String("private detail".into()),
            ),
        ]));
        assert_eq!(
            public_route_loader_failure(&source),
            plec_ir::PublicRouteLoaderFailure::generic()
        );
    }

    #[test]
    fn network_and_decode_failures_round_trip_through_the_public_record() {
        for kind in ["network", "decode"] {
            let record = RuntimeValue::Record(std::collections::HashMap::from([
                ("kind".into(), RuntimeValue::String(kind.into())),
                (
                    "message".into(),
                    RuntimeValue::String(format!("{kind} failed")),
                ),
                ("url".into(), RuntimeValue::String("/api/data".into())),
            ]));
            let public = public_route_loader_failure(&record);
            assert_eq!(route_loader_failure_value(&public), record);
        }
    }
}
