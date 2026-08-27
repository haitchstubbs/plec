struct ErrorInfo {
    pub code: u16,
    pub message: &'static str,
}

impl ErrorInfo {
    fn info(self) -> ErrorInfo {
        match self {
            ErrorCode::UnsupportedFetchDecoder => ErrorInfo {
                code: 0001,
                message: "Unsupported fetch decoder",
            },
            ErrorCode::UnsupportedFetchMethod => ErrorInfo {
                code: 0002,
                message: "Unsupported fetch method",
            },
            ErrorCode::UncallableActionProp => ErrorInfo {
                code: 0003,
                message: "Action is not a callable component prop",
            },
            ErrorCode::UncallableProp => ErrorInfo {
                code: 0004,
                message: "Callable component prop must be a direct binding",
            },
            ErrorCode::IndirectlyCallableAction => ErrorInfo {
                code: 0005,
                message: "Callable action must be a direct component prop call",
            },
        }
    }
}
