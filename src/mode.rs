use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// Every fan mode the privileged helper is allowed to apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FanMode {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "1")]
    Level1,
    #[serde(rename = "2")]
    Level2,
    #[serde(rename = "3")]
    Level3,
    #[serde(rename = "4")]
    Level4,
    #[serde(rename = "5")]
    Level5,
    #[serde(rename = "6")]
    Level6,
    #[serde(rename = "7")]
    Level7,
    #[serde(rename = "full-speed")]
    FullSpeed,
}

impl FanMode {
    pub const ALL: [Self; 9] = [
        Self::Auto,
        Self::Level1,
        Self::Level2,
        Self::Level3,
        Self::Level4,
        Self::Level5,
        Self::Level6,
        Self::Level7,
        Self::FullSpeed,
    ];

    pub const fn kernel_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Level1 => "1",
            Self::Level2 => "2",
            Self::Level3 => "3",
            Self::Level4 => "4",
            Self::Level5 => "5",
            Self::Level6 => "6",
            Self::Level7 => "7",
            Self::FullSpeed => "full-speed",
        }
    }

    pub const fn menu_label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic (recommended)",
            Self::Level1 => "Manual level 1",
            Self::Level2 => "Manual level 2",
            Self::Level3 => "Manual level 3",
            Self::Level4 => "Manual level 4",
            Self::Level5 => "Manual level 5",
            Self::Level6 => "Manual level 6",
            Self::Level7 => "Manual level 7",
            Self::FullSpeed => "Full speed (maximum cooling)",
        }
    }

    pub const fn is_manual(self) -> bool {
        !matches!(self, Self::Auto)
    }
}

impl fmt::Display for FanMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.kernel_value())
    }
}

impl FromStr for FanMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.kernel_value() == value)
            .ok_or("Invalid fan mode.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_safe_modes_parse() {
        for value in ["auto", "1", "2", "3", "4", "5", "6", "7", "full-speed"] {
            assert!(value.parse::<FanMode>().is_ok());
        }
        for value in ["0", "8", "99", "disengaged", "level 7", ""] {
            assert!(value.parse::<FanMode>().is_err(), "accepted {value:?}");
        }
    }
}
