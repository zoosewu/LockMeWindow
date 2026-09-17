use crate::processes::process_identity;
use window_warden::{AudioSession, MuteActions};
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IAudioSessionControl, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::core::Interface;

pub struct AudioSessions {
    devices: IMMDeviceEnumerator,
}

// The sessions seen in one pass, with the volume control each was read from.
#[derive(Default)]
pub struct Snapshot {
    pub sessions: Vec<AudioSession>,
    volumes: Vec<ISimpleAudioVolume>,
}

impl AudioSessions {
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            // The UI thread may already belong to an apartment; COM is usable either way.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            Ok(Self {
                devices: CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?,
            })
        }
    }

    // Every session on every active playback device, except system sounds.
    pub fn snapshot(&self) -> Snapshot {
        let mut snapshot = Snapshot::default();
        unsafe {
            let Ok(devices) = self
                .devices
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            else {
                return snapshot;
            };
            for index in 0..devices.GetCount().unwrap_or(0) {
                let Ok(manager) = devices
                    .Item(index)
                    .and_then(|device| device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None))
                else {
                    continue;
                };
                let Ok(sessions) = manager.GetSessionEnumerator() else {
                    continue;
                };
                for position in 0..sessions.GetCount().unwrap_or(0) {
                    if let Some((session, volume)) = sessions
                        .GetSession(position)
                        .ok()
                        .and_then(|control| read_session(&control))
                    {
                        snapshot.sessions.push(session);
                        snapshot.volumes.push(volume);
                    }
                }
            }
        }
        snapshot
    }
}

impl Snapshot {
    pub fn apply(&self, actions: &MuteActions) {
        for (session, volume) in self.sessions.iter().zip(&self.volumes) {
            let mute = if actions.mute.contains(&session.key) {
                true
            } else if actions.unmute.contains(&session.key) {
                false
            } else {
                continue;
            };
            unsafe {
                let _ = volume.SetMute(mute, std::ptr::null());
            }
        }
    }
}

unsafe fn read_session(
    control: &IAudioSessionControl,
) -> Option<(AudioSession, ISimpleAudioVolume)> {
    let control: IAudioSessionControl2 = control.cast().ok()?;
    if control.IsSystemSoundsSession() == S_OK {
        return None;
    }
    let pid = control.GetProcessId().ok().filter(|pid| *pid != 0)?;
    let identity = process_identity(pid)?;

    let instance = control.GetSessionInstanceIdentifier().ok()?;
    let key = instance.to_string().ok();
    CoTaskMemFree(Some(instance.0 as *const _));

    let volume: ISimpleAudioVolume = control.cast().ok()?;
    let muted = volume.GetMute().ok()?.as_bool();
    Some((
        AudioSession {
            key: key?,
            process_name: identity.name,
            process_path: identity.path,
            muted,
        },
        volume,
    ))
}
