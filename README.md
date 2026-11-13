# Fan Speed Control

A native Rust/GTK application for monitoring and manually controlling the fans
on supported Linux ThinkPads and Intel MacBooks. It provides:

- automatic firmware control;
- manual fan levels 1 through 7;
- full-speed cooling;
- live fan RPM and CPU/GPU temperature readings;
- a Linux status-bar menu for changing modes without opening the window;
- one PolicyKit authentication prompt per application session.

The desktop application runs as the normal user. A narrow, root-owned Rust
helper accepts only the validated modes `auto`, `1` through `7`, and
`full-speed`. It communicates with the application over private inherited
standard-I/O pipes and exits with the application.

## Requirements

- Linux on a ThinkPad with the `thinkpad_acpi` fan interface, or an Intel MacBook
  with the `applesmc` fan interface;
- Rust 1.85 or newer with Cargo;
- GTK 3 development files;
- Ayatana AppIndicator 3 development files;
- PolicyKit (`pkexec`).

On Ubuntu/Debian, `install.sh` offers to install the build dependencies when
they are missing.

## Install

Run the installer as your normal user (not through `sudo`):

```bash
chmod +x install.sh uninstall.sh build-appimage.sh packaging/appimage/AppRun
./install.sh
```

The installer builds an optimized binary, then uses `sudo` only to install the
application, helper, launcher, icon, policy, and uninstaller. Open **Fan Speed
Control** from the launcher or run:

```bash
fanspeed
```

## Run from source

Install the privileged helper once with `./install.sh`, then run a development
build with:

```bash
cargo run --bin fanspeed
```

Run the test suite with:

```bash
cargo test --all-targets
```

The tests use temporary fake `/proc` and `/sys` trees; they do not change fan
state or require root.

## AppImage

Build the AppImage in an Ubuntu 22.04 container:

```bash
./build-appimage.sh
```

The artifact is written to `dist/ThinkPad_Fan_Control-x86_64.AppImage`. The
AppImage includes the GUI and the same narrow helper entry point. PolicyKit
authenticates once when the application starts; closing the application ends
the helper session.

## Status-bar controls

The status-bar label displays the current mode: `A` for automatic, `1` through
`7` for a manual level, and `MAX` for full speed. Its menu includes live
temperatures/RPM, all fan modes, **Open Fan Control**, and **Quit**.

Closing the main window hides it while the status-bar process keeps running.
Choose **Quit** to end the application completely.

## How fan control works

The helper selects a driver from the fan interfaces exposed by the system. On
ThinkPads, it sends commands through `thinkpad_acpi` to the embedded controller.
Linux normally disables manual commands. On the first manual selection, the
helper reloads the module with:

```bash
modprobe thinkpad_acpi fan_control=1
```

It then writes one of these values to `/proc/acpi/ibm/fan`:

```text
level 1
level 2
...
level 7
level full-speed
level auto
```

Levels are controller-defined cooling levels, not exact percentages or RPMs.
`full-speed` may be faster than level 7. `auto` returns control to firmware.

On Intel MacBooks with `applesmc`, the helper writes a target RPM to
`/sys/devices/platform/applesmc.768/fan1_output` and selects manual mode through
`fan1_manual`. Levels 1 through 7 are evenly spaced between the reported
minimum and maximum RPM; **Full speed** targets the reported maximum. Automatic
mode writes `0` to `fan1_manual`, returning control to firmware. The displayed
RPM is measured speed and may differ from the target.

If no supported interface is found, check the machine identity and driver:

```bash
cat /sys/class/dmi/id/sys_vendor /sys/class/dmi/id/product_name
ls /proc/acpi/ibm/fan /sys/devices/platform/applesmc.768/fan1_manual
```

The helper does not try to load `thinkpad_acpi` on a machine without a ThinkPad
fan interface or ThinkPad system identity.

## Safety

Manual mode may remain active after the program closes. Select **Automatic
(recommended)** when manual cooling is no longer needed. A reboot also restores
automatic control on ThinkPads because this application does not persist the
module option.

The typed Rust control module cannot represent level 0, and the helper rejects
unknown fields and invalid modes before touching the kernel interface.

## Uninstall

```bash
sudo fanspeed-uninstall
```

The uninstaller stops the application, attempts to restore automatic firmware
control, and removes all installed application files. Shared GTK/AppIndicator
packages are retained.

