use rodio::cpal::{
    self,
    traits::{DeviceTrait, HostTrait},
};

/// An output device for feedback sounds. Microphones are listed by
/// [`list_microphones`].
pub struct OutputDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
}

/// DanyWhere: a microphone as the app uses it, from
/// `handy_recorder::list_input_devices`. handy-recorder's `InputDevice`
/// can't be built outside it, so tests list these instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrophoneDevice {
    /// What `RecorderConfig::device` opens.
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// Channels at the device's format; `None` where reading them would
    /// open the device (ALSA).
    pub channels: Option<u16>,
    /// A PulseAudio monitor source, which records a speaker's output.
    pub is_monitor: bool,
}

impl From<handy_recorder::InputDevice> for MicrophoneDevice {
    fn from(device: handy_recorder::InputDevice) -> Self {
        Self {
            id: device.id,
            name: device.name,
            is_default: device.is_default,
            channels: device.channels,
            is_monitor: device.is_monitor,
        }
    }
}

/// DanyWhere: the system's microphones now.
pub fn list_microphones() -> Result<Vec<MicrophoneDevice>, anyhow::Error> {
    Ok(handy_recorder::list_input_devices()?
        .into_iter()
        .map(MicrophoneDevice::from)
        .collect())
}

/// DanyWhere: where feedback sounds and the settings find output devices:
/// the system's ([`SystemOutputHost`]) or any `cpal::Host`, such as a fake
/// one in tests (`fake_output`).
pub trait OutputHost {
    /// The device a sound plays on when none is selected or the selected one
    /// is missing.
    fn default_device(&self) -> Option<cpal::Device>;
    /// Every output device, in the host's order.
    fn all_devices(&self) -> Result<Box<dyn Iterator<Item = cpal::Device>>, cpal::Error>;
}

impl OutputHost for cpal::Host {
    fn default_device(&self) -> Option<cpal::Device> {
        self.default_output_device()
    }

    fn all_devices(&self) -> Result<Box<dyn Iterator<Item = cpal::Device>>, cpal::Error> {
        Ok(Box::new(self.output_devices()?))
    }
}

/// DanyWhere: the system's output devices as feedback sounds have always
/// found them: the default through cpal's default host, as rodio's
/// `DeviceSinkBuilder::from_default_device` does, a selected device by name
/// through `get_cpal_host` (ALSA on Linux).
pub struct SystemOutputHost;

impl OutputHost for SystemOutputHost {
    fn default_device(&self) -> Option<cpal::Device> {
        cpal::default_host().default_output_device()
    }

    fn all_devices(&self) -> Result<Box<dyn Iterator<Item = cpal::Device>>, cpal::Error> {
        crate::audio_toolkit::get_cpal_host().all_devices()
    }
}

/// The name feedback sounds match `selected_output_device` against.
pub fn output_device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_owned())
}

pub fn list_output_devices() -> Result<Vec<OutputDeviceInfo>, Box<dyn std::error::Error>> {
    list_output_devices_on(&crate::audio_toolkit::get_cpal_host())
}

/// DanyWhere: the output devices of `host`, numbered in its order. The
/// default is told by its name.
pub fn list_output_devices_on(
    host: &impl OutputHost,
) -> Result<Vec<OutputDeviceInfo>, Box<dyn std::error::Error>> {
    let default_name = host.default_device().and_then(|d| output_device_name(&d));

    let mut out = Vec::<OutputDeviceInfo>::new();

    for (index, device) in host.all_devices()?.enumerate() {
        let name = output_device_name(&device).unwrap_or_else(|| "Unknown".into());

        let is_default = Some(name.clone()) == default_name;

        out.push(OutputDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
        });
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::audio::fake_output::{FakeOutput, FakeOutputs};
    use rodio::cpal::ErrorKind;

    // DanyWhere: the list of output devices on a fake host (`fake_output`).

    /// The list as (index, name, is the default).
    fn listed(outputs: &FakeOutputs) -> Vec<(String, String, bool)> {
        list_output_devices_on(&outputs.host())
            .expect("the devices are listed")
            .into_iter()
            .map(|device| (device.index, device.name, device.is_default))
            .collect()
    }

    fn row(index: &str, name: &str, is_default: bool) -> (String, String, bool) {
        (index.to_string(), name.to_string(), is_default)
    }

    #[test]
    fn the_list_numbers_the_devices_in_order_and_marks_the_default() {
        let speakers = FakeOutput::new("MacBook Pro Speakers");
        let headset = FakeOutput::new("USB Headset");
        let display = FakeOutput::new("Studio Display Speakers");
        let outputs = FakeOutputs::new(&[&speakers, &headset, &display]).with_default(&headset);
        assert_eq!(
            listed(&outputs),
            [
                row("0", "MacBook Pro Speakers", false),
                row("1", "USB Headset", true),
                row("2", "Studio Display Speakers", false),
            ]
        );
    }

    #[test]
    fn without_a_default_device_none_is_marked() {
        let speakers = FakeOutput::new("MacBook Pro Speakers");
        let outputs = FakeOutputs::new(&[&speakers]);
        assert_eq!(listed(&outputs), [row("0", "MacBook Pro Speakers", false)]);
    }

    #[test]
    fn a_device_whose_name_cannot_be_read_is_listed_as_unknown() {
        let speakers = FakeOutput::new("MacBook Pro Speakers");
        let gone = FakeOutput::new("AirPods Pro").nameless();
        let outputs = FakeOutputs::new(&[&speakers, &gone]).with_default(&speakers);
        assert_eq!(
            listed(&outputs),
            [
                row("0", "MacBook Pro Speakers", true),
                row("1", "Unknown", false),
            ]
        );
    }

    #[test]
    fn a_default_whose_name_cannot_be_read_marks_no_device() {
        let speakers = FakeOutput::new("MacBook Pro Speakers").nameless();
        let headset = FakeOutput::new("USB Headset");
        let outputs = FakeOutputs::new(&[&speakers, &headset]).with_default(&speakers);
        assert_eq!(
            listed(&outputs),
            [row("0", "Unknown", false), row("1", "USB Headset", false)]
        );
    }

    #[test]
    fn devices_named_as_the_default_are_all_marked_default() {
        // Today's behaviour: the default is told by its name, so a second
        // device of that name looks like the default too.
        let first = FakeOutput::new("USB Audio");
        let second = FakeOutput::new("USB Audio");
        let outputs = FakeOutputs::new(&[&first, &second]).with_default(&first);
        assert_eq!(
            listed(&outputs),
            [row("0", "USB Audio", true), row("1", "USB Audio", true)]
        );
    }

    #[test]
    fn a_microphone_is_not_listed() {
        let speakers = FakeOutput::new("MacBook Pro Speakers");
        let microphone = FakeOutput::new("MacBook Pro Microphone").input_only();
        let outputs = FakeOutputs::new(&[&microphone, &speakers]).with_default(&speakers);
        assert_eq!(listed(&outputs), [row("0", "MacBook Pro Speakers", true)]);
    }

    #[test]
    fn a_list_that_cannot_be_read_is_an_error() {
        let speakers = FakeOutput::new("MacBook Pro Speakers");
        let outputs = FakeOutputs::new(&[&speakers])
            .with_default(&speakers)
            .failing_to_list(ErrorKind::HostUnavailable);
        let error = list_output_devices_on(&outputs.host())
            .err()
            .expect("the list fails");
        assert_eq!(error.to_string(), ErrorKind::HostUnavailable.to_string());
    }
}
