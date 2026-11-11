use crate::sensors::{CurrentMode, FanState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrayStatus {
    pub short_mode: String,
    pub details: String,
    pub selected_mode: Option<String>,
}

pub fn tray_status(state: &FanState) -> TrayStatus {
    let (short_mode, selected_mode) = match &state.mode {
        CurrentMode::Automatic => ("A".to_owned(), Some("auto".to_owned())),
        CurrentMode::FullSpeed => ("MAX".to_owned(), Some("full-speed".to_owned())),
        CurrentMode::Manual(level)
            if matches!(level.as_str(), "1" | "2" | "3" | "4" | "5" | "6" | "7") =>
        {
            (level.clone(), Some(level.clone()))
        }
        _ => ("?".to_owned(), None),
    };
    let mut details = vec![state.mode.label()];
    if !state.rpms.is_empty() {
        details.push(
            state
                .rpms
                .iter()
                .map(|rpm| format_number(*rpm))
                .collect::<Vec<_>>()
                .join(" / "),
        );
    }
    let mut temperatures = Vec::new();
    if let Some(value) = state.cpu_temp {
        temperatures.push(format!("CPU {value:.1}°C"));
    }
    if let Some(value) = state.gpu_temp {
        temperatures.push(format!("GPU {value:.1}°C"));
    }
    if !temperatures.is_empty() {
        details.push(temperatures.join(" / "));
    }
    TrayStatus {
        short_mode,
        details: details.join("  •  "),
        selected_mode,
    }
}

pub fn format_number(value: u32) -> String {
    let digits = value.to_string();
    let groups = digits.as_bytes().rchunks(3).collect::<Vec<_>>();
    let grouped = groups
        .into_iter()
        .rev()
        .map(|group| std::str::from_utf8(group).expect("decimal digits are UTF-8"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{grouped} RPM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_automatic_status() {
        let status = tray_status(&FanState {
            mode: CurrentMode::Automatic,
            rpms: vec![2400, 2300],
            cpu_temp: Some(57.5),
            gpu_temp: None,
        });
        assert_eq!(status.short_mode, "A");
        assert_eq!(status.selected_mode.as_deref(), Some("auto"));
        assert!(status.details.contains("2,400 RPM / 2,300 RPM"));
        assert!(status.details.contains("CPU 57.5°C"));
    }

    #[test]
    fn formats_manual_and_full_speed_modes() {
        let base = |mode| FanState {
            mode,
            rpms: vec![],
            cpu_temp: None,
            gpu_temp: None,
        };
        assert_eq!(
            tray_status(&base(CurrentMode::Manual("4".into()))).short_mode,
            "4"
        );
        assert_eq!(tray_status(&base(CurrentMode::FullSpeed)).short_mode, "MAX");
    }
}
