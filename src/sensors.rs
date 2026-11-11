use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub enum CurrentMode {
    Automatic,
    Manual(String),
    FullSpeed,
    Unavailable,
}

impl CurrentMode {
    pub fn label(&self) -> String {
        match self {
            Self::Automatic => "Automatic".to_owned(),
            Self::Manual(level) => format!("Manual level {level}"),
            Self::FullSpeed => "Full speed".to_owned(),
            Self::Unavailable => "Unavailable".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FanState {
    pub mode: CurrentMode,
    pub rpms: Vec<u32>,
    pub cpu_temp: Option<f64>,
    pub gpu_temp: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct SensorPaths {
    pub proc_fan: PathBuf,
    pub hwmon_root: PathBuf,
}

impl Default for SensorPaths {
    fn default() -> Self {
        Self {
            proc_fan: "/proc/acpi/ibm/fan".into(),
            hwmon_root: "/sys/class/hwmon".into(),
        }
    }
}

/// Reads all monitoring data in one best-effort snapshot.
#[derive(Clone, Debug)]
pub struct SensorReader {
    paths: SensorPaths,
}

impl Default for SensorReader {
    fn default() -> Self {
        Self::new(SensorPaths::default())
    }
}

impl SensorReader {
    pub fn new(paths: SensorPaths) -> Self {
        Self { paths }
    }

    pub fn read(&self) -> FanState {
        let mode = read_text(&self.paths.proc_fan)
            .as_deref()
            .and_then(parse_level)
            .unwrap_or(CurrentMode::Unavailable);
        let rpms = self
            .find_hwmon("thinkpad")
            .map(|directory| {
                ["fan1_input", "fan2_input"]
                    .into_iter()
                    .filter_map(|name| read_text(&directory.join(name))?.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let cpu_temp = self.read_temperature("k10temp");
        let gpu_temp = self.read_temperature("amdgpu");
        FanState {
            mode,
            rpms,
            cpu_temp,
            gpu_temp,
        }
    }

    fn find_hwmon(&self, expected_name: &str) -> Option<PathBuf> {
        fs::read_dir(&self.paths.hwmon_root)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| read_text(&path.join("name")).as_deref() == Some(expected_name))
    }

    fn read_temperature(&self, hwmon_name: &str) -> Option<f64> {
        let value = read_text(&self.find_hwmon(hwmon_name)?.join("temp1_input"))?;
        value.parse::<i64>().ok().map(|value| value as f64 / 1000.0)
    }
}

fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn parse_level(contents: &str) -> Option<CurrentMode> {
    let value = contents.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "level").then(|| value.trim())
    })?;
    Some(match value {
        "auto" => CurrentMode::Automatic,
        "full-speed" | "disengaged" => CurrentMode::FullSpeed,
        value if !value.is_empty() => CurrentMode::Manual(value.to_owned()),
        _ => CurrentMode::Unavailable,
    })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn reads_proc_and_hwmon_snapshot() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fanspeed-sensors-{suffix}"));
        let hwmon = root.join("hwmon");
        fs::create_dir_all(hwmon.join("hwmon0")).unwrap();
        fs::create_dir_all(hwmon.join("hwmon1")).unwrap();
        fs::write(root.join("fan"), "status:\tenabled\nlevel:\t4\n").unwrap();
        fs::write(hwmon.join("hwmon0/name"), "thinkpad\n").unwrap();
        fs::write(hwmon.join("hwmon0/fan1_input"), "2400\n").unwrap();
        fs::write(hwmon.join("hwmon1/name"), "k10temp\n").unwrap();
        fs::write(hwmon.join("hwmon1/temp1_input"), "57500\n").unwrap();
        let state = SensorReader::new(SensorPaths {
            proc_fan: root.join("fan"),
            hwmon_root: hwmon,
        })
        .read();
        assert_eq!(state.mode, CurrentMode::Manual("4".to_owned()));
        assert_eq!(state.rpms, vec![2400]);
        assert_eq!(state.cpu_temp, Some(57.5));
        assert_eq!(state.gpu_temp, None);
        fs::remove_dir_all(root).unwrap();
    }
}
