pub use plec_lowering::LoweringError;

pub fn lower_component_to_executable(
    component: &plec_hir::HirComponent,
) -> Result<plec_ir::ExecutableApplication, LoweringError> {
    plec_lowering::component::lower_component_to_executable(component)
}

pub fn lower_route_loader_to_executable(
    component: &plec_hir::HirComponent,
    loader_name: &str,
    result_state_name: &str,
    outlet_id: &str,
) -> Result<plec_ir::ExecutableApplication, LoweringError> {
    plec_lowering::component::lower_route_loader_to_executable(
        component,
        loader_name,
        result_state_name,
        outlet_id,
    )
}

pub fn lower_application_to_executable(
    application: &plec_hir::HirApplication,
) -> Result<plec_ir::ComponentApplication, LoweringError> {
    plec_lowering::application::lower_application_to_executable(application)
}
