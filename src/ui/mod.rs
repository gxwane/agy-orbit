pub mod banner;
pub mod completion;
pub mod console;
pub mod doctor_view;
pub mod quota_view;
pub mod selector;
pub mod table_view;
pub mod uninstall_view;
pub mod upgrade_view;

pub use banner::{render_success, render_whoami};
pub use completion::{detect_current_shell, emit_completion_script, render_completion_guide};
pub use console::init_terminal_colors;
pub use doctor_view::render_doctor_report;
pub use quota_view::{render_multi_quota_table, render_quota_tip_if_multiple, render_quota_view};
pub use selector::{install_terminal_panic_hook, is_interactive, select_orbit_interactive};
pub use table_view::render_orbits_table;
pub use uninstall_view::{render_uninstall_plan, render_uninstall_result};
pub use upgrade_view::{
    is_cargo_installation, is_command_whitelisted_for_update_check, render_update_hint,
    render_upgrade_result, should_enable_startup_update_check,
};
