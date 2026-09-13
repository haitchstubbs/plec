pub fn sanitize(id: &str) -> String {
    id.replace(['/', '\\'], "--").replace('#', "--")
}
