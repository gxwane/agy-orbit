pub mod banner;
pub mod completion;
pub mod quota_view;
pub mod selector;
pub mod table_view;
pub mod uninstall_view;
pub mod upgrade_view;

pub use banner::{render_success, render_whoami};
pub use completion::{detect_current_shell, emit_completion_script, render_completion_guide};
pub use quota_view::{render_multi_quota_table, render_quota_tip_if_multiple, render_quota_view};
pub use selector::{install_terminal_panic_hook, is_interactive, select_orbit_interactive};
pub use table_view::render_orbits_table;
pub use uninstall_view::{render_uninstall_plan, render_uninstall_result};
pub use upgrade_view::render_upgrade_result;
