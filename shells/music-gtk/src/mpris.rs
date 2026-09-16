//! MPRIS D-Bus host adapter.
//!
//! It exports only remote-control commands and display metadata. Playback
//! queues and core domain records never cross D-Bus or UniFFI.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use glib::variant::ToVariant;
use gtk::gio;
use gtk::glib;

use crate::TransportSnapshot;

const BUS_NAME: &str = "org.mpris.MediaPlayer2.LocalMusic";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

const INTROSPECTION: &str = r#"
<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/>
    <method name="Previous"/>
    <method name="Pause"/>
    <method name="PlayPause"/>
    <method name="Stop"/>
    <method name="Play"/>
    <method name="Seek"><arg direction="in" name="Offset" type="x"/></method>
    <method name="SetPosition">
      <arg direction="in" name="TrackId" type="o"/>
      <arg direction="in" name="Position" type="x"/>
    </method>
    <method name="OpenUri"><arg direction="in" name="Uri" type="s"/></method>
    <signal name="Seeked"><arg name="Position" type="x"/></signal>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="LoopStatus" type="s" access="readwrite"/>
    <property name="Rate" type="d" access="readwrite"/>
    <property name="Shuffle" type="b" access="readwrite"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Volume" type="d" access="readwrite"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
</node>
"#;

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteCommand {
    Raise,
    Quit,
    Next,
    Previous,
    Pause,
    PlayPause,
    Stop,
    Play,
    Seek(i64),
    SetPosition(u64),
    SetVolume(f64),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MprisState {
    pub transport: TransportSnapshot,
    pub title: Option<String>,
}

/// Name ownership keeps the real D-Bus object alive until the window drops.
pub struct MprisHost {
    owner_id: Option<gio::OwnerId>,
    connection: Rc<RefCell<Option<gio::DBusConnection>>>,
    state: Rc<dyn Fn() -> MprisState>,
}

impl MprisHost {
    #[must_use]
    pub fn start(command: Rc<dyn Fn(RemoteCommand)>, state: Rc<dyn Fn() -> MprisState>) -> Self {
        let connection_slot = Rc::new(RefCell::new(None));
        let acquired_slot = connection_slot.clone();
        let acquired_command = command.clone();
        let acquired_state = state.clone();
        let owner_id = gio::bus_own_name(
            gio::BusType::Session,
            BUS_NAME,
            gio::BusNameOwnerFlags::NONE,
            move |connection, _| {
                acquired_slot.replace(Some(connection.clone()));
                register_interfaces(connection, acquired_command.clone(), acquired_state.clone());
            },
            |_, _| {},
            |_, _| {},
        );
        Self {
            owner_id: Some(owner_id),
            connection: connection_slot,
            state,
        }
    }

    /// Emit standard property invalidation after a local or remote command.
    pub fn notify_player(&self) {
        let Some(connection) = self.connection.borrow().as_ref().cloned() else {
            return;
        };
        let state = (self.state)();
        let changed = player_properties(&state);
        let parameters = (PLAYER_INTERFACE, changed, Vec::<String>::new()).to_variant();
        let _ = connection.emit_signal(
            None,
            OBJECT_PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(&parameters),
        );
    }
}

impl Drop for MprisHost {
    fn drop(&mut self) {
        if let Some(owner_id) = self.owner_id.take() {
            gio::bus_unown_name(owner_id);
        }
    }
}

fn register_interfaces(
    connection: gio::DBusConnection,
    command: Rc<dyn Fn(RemoteCommand)>,
    state: Rc<dyn Fn() -> MprisState>,
) {
    let Ok(node) = gio::DBusNodeInfo::for_xml(INTROSPECTION) else {
        return;
    };
    let Some(root) = node.lookup_interface(ROOT_INTERFACE) else {
        return;
    };
    let root_command = command.clone();
    let _ = connection
        .register_object(OBJECT_PATH, &root)
        .method_call(move |_, _, _, _, method, _, invocation| {
            let remote = match method {
                "Raise" => Some(RemoteCommand::Raise),
                "Quit" => Some(RemoteCommand::Quit),
                _ => None,
            };
            if let Some(remote) = remote {
                root_command(remote);
                invocation.return_value(Some(&().to_variant()));
            } else {
                invocation.return_dbus_error(
                    "org.mpris.MediaPlayer2.Error.NotSupported",
                    "Unsupported root command",
                );
            }
        })
        .property(move |_, _, _, _, property| root_property(property))
        .set_property(|_, _, _, _, _, _| false)
        .build();

    let Some(player) = node.lookup_interface(PLAYER_INTERFACE) else {
        return;
    };
    let method_command = command.clone();
    let property_state = state.clone();
    let set_command = command;
    let _ = connection
        .register_object(OBJECT_PATH, &player)
        .method_call(move |_, _, _, _, method, parameters, invocation| {
            let remote = match method {
                "Next" => Some(RemoteCommand::Next),
                "Previous" => Some(RemoteCommand::Previous),
                "Pause" => Some(RemoteCommand::Pause),
                "PlayPause" => Some(RemoteCommand::PlayPause),
                "Stop" => Some(RemoteCommand::Stop),
                "Play" => Some(RemoteCommand::Play),
                "Seek" => parameters
                    .child_value(0)
                    .get::<i64>()
                    .map(|offset| RemoteCommand::Seek(offset / 1_000)),
                "SetPosition" => parameters.child_value(1).get::<i64>().and_then(|position| {
                    u64::try_from(position / 1_000)
                        .ok()
                        .map(RemoteCommand::SetPosition)
                }),
                "OpenUri" => None,
                _ => None,
            };
            if let Some(remote) = remote {
                method_command(remote);
                invocation.return_value(Some(&().to_variant()));
            } else {
                invocation.return_dbus_error(
                    "org.mpris.MediaPlayer2.Player.Error.NotSupported",
                    "Opening arbitrary URIs is not supported",
                );
            }
        })
        .property(move |_, _, _, _, property| player_property(property, &(property_state)()))
        .set_property(move |_, _, _, _, property, value| {
            if property == "Volume" {
                if let Some(volume) = value.get::<f64>() {
                    set_command(RemoteCommand::SetVolume(volume));
                    return true;
                }
            }
            false
        })
        .build();
}

fn root_property(name: &str) -> glib::Variant {
    match name {
        "CanQuit" => false.to_variant(),
        "CanRaise" => true.to_variant(),
        "HasTrackList" => false.to_variant(),
        "Identity" => "LocalMusic".to_variant(),
        "DesktopEntry" => "localmusic".to_variant(),
        "SupportedUriSchemes" => vec!["file".to_owned()].to_variant(),
        "SupportedMimeTypes" => Vec::<String>::new().to_variant(),
        _ => ().to_variant(),
    }
}

fn player_property(name: &str, state: &MprisState) -> glib::Variant {
    match name {
        "PlaybackStatus" => state.transport.status.mpris_name().to_variant(),
        "LoopStatus" => "None".to_variant(),
        "Rate" | "MinimumRate" | "MaximumRate" => 1.0f64.to_variant(),
        "Shuffle" => false.to_variant(),
        "Metadata" => metadata(state).to_variant(),
        "Volume" => state.transport.volume.to_variant(),
        "Position" => millis_to_micros(state.transport.position_ms).to_variant(),
        "CanGoNext" | "CanGoPrevious" | "CanPlay" | "CanPause" | "CanSeek" | "CanControl" => {
            true.to_variant()
        }
        _ => ().to_variant(),
    }
}

fn player_properties(state: &MprisState) -> HashMap<String, glib::Variant> {
    [
        (
            "PlaybackStatus".into(),
            state.transport.status.mpris_name().to_variant(),
        ),
        ("Metadata".into(), metadata(state).to_variant()),
        ("Volume".into(), state.transport.volume.to_variant()),
        (
            "Position".into(),
            millis_to_micros(state.transport.position_ms).to_variant(),
        ),
    ]
    .into_iter()
    .collect()
}

fn metadata(state: &MprisState) -> HashMap<String, glib::Variant> {
    let mut metadata = HashMap::new();
    if let Some(title) = &state.title {
        metadata.insert("xesam:title".into(), title.to_variant());
    }
    metadata
}

fn millis_to_micros(milliseconds: u64) -> i64 {
    i64::try_from(milliseconds.saturating_mul(1_000)).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpris_contract_is_parseable_and_uses_display_metadata() {
        let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).unwrap();
        assert!(node.lookup_interface(ROOT_INTERFACE).is_some());
        assert!(node.lookup_interface(PLAYER_INTERFACE).is_some());
        let state = MprisState {
            transport: TransportSnapshot {
                status: crate::PlaybackStatus::Playing,
                position_ms: 12_345,
                volume: 0.5,
            },
            title: Some("Track".into()),
        };
        assert_eq!(
            player_property("PlaybackStatus", &state).get::<String>(),
            Some("Playing".into())
        );
        assert_eq!(
            player_property("Position", &state).get::<i64>(),
            Some(12_345_000)
        );
        assert!(metadata(&state).contains_key("xesam:title"));
    }
}
