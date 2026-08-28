/*
target api:
let inspector = Inspector::new(&application);

let result = inspector
    .query("{ components { id nodeCount bindingCount } }")
    .await;
*/
mod schema;

pub use schema::Inspector;
