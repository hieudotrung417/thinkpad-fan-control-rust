//! Core modules for supported laptop fan control.

pub mod control;
pub mod helper;
pub mod mode;
pub mod sensors;
pub mod session;
pub mod status;

pub const APP_ID: &str = "io.github.fanspeed.FanSpeed";
pub const HELPER_PATH: &str = "/usr/local/libexec/fanspeed-helper";
pub const PKEXEC_PATH: &str = "/usr/bin/pkexec";
