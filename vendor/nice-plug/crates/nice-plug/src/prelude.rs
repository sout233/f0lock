// Used in [`AudioIOLayout`]
pub use std::num::NonZeroU32;

// Re-export the macros, derive macros are already re-exported from their respective modules
pub use crate::debug::*;

pub use crate::nice_export_clap;
#[cfg(feature = "vst3")]
pub use crate::nice_export_vst3;
#[cfg(feature = "standalone")]
pub use crate::wrapper::standalone::{nice_export_standalone, nice_export_standalone_with_args};

pub use crate::formatters;
pub use crate::util;

pub use crate::audio_setup::{
    AudioIOLayout, AuxiliaryBuffers, BufferConfig, PortNames, ProcessMode, new_nonzero_u32,
};
pub use crate::buffer::Buffer;
pub use crate::context::PluginApi;
pub use crate::context::activate::ActivateContext;
#[cfg(feature = "editor")]
pub use crate::context::gui::{AsyncExecutor, GuiContextInner, ParamSetter};
pub use crate::context::process::{ProcessContext, Transport};
pub use crate::context::remote_controls::{
    RemoteControlsContext, RemoteControlsPage, RemoteControlsSection,
};
#[cfg(feature = "editor")]
pub use crate::editor::{Editor, Modifiers, ParentWindowHandle, ResizeHint, VirtualKeyCode};
pub use crate::midi::sysex::SysExMessage;
pub use crate::midi::{MidiConfig, NoteEvent, PluginNoteEvent, control_change};
pub use crate::params::Params;
pub use crate::params::enums::{Enum, EnumParam};
pub use crate::params::internals::ParamPtr;
pub use crate::params::range::{FloatRange, IntRange};
pub use crate::params::smoothing::{AtomicF32, Smoothable, Smoother, SmoothingStyle};
pub use crate::params::{BoolParam, FloatParam, IntParam, Param, ParamFlags};
pub use crate::plugin::PluginState;
pub use crate::plugin::{Plugin, ProcessStatus, TaskExecutor};
#[cfg(feature = "editor")]
pub use crate::plugin::{TrackColor, TrackInfo};
pub use crate::wrapper::clap::features::ClapFeature;
pub use crate::wrapper::clap::{ClapPlugin, PolyModulationConfig};
#[cfg(feature = "vst3")]
pub use crate::wrapper::vst3::Vst3Plugin;
#[cfg(feature = "vst3")]
pub use crate::wrapper::vst3::subcategories::Vst3SubCategory;
