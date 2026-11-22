use rodio::cpal::{
    self,
    traits::{DeviceTrait, HostTrait},
};

/// An output device for feedback sounds. Microphones are listed by
/// `handy_recorder::list_input_devices`.
pub struct OutputDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
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
    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host
        .default_output_device()
        .and_then(|d| output_device_name(&d));

    let mut out = Vec::<OutputDeviceInfo>::new();

    for (index, device) in host.output_devices()?.enumerate() {
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
