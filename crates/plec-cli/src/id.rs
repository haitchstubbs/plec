pub fn sanitize(graph_id: &str) -> String {
    graph_id.replace(['/', '\\'], "--").replace('#', "--")
}
