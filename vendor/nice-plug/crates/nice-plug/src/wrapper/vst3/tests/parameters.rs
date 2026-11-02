use super::*;
use crate::prelude::*;
use std::sync::{Mutex, Weak};
use vst3::{
    ComWrapper,
    Steinberg::Vst::{IComponentHandler, IComponentHandlerTrait},
};

#[derive(Params)]
struct TestParams {
    #[id = "gain"]
    gain: FloatParam,
    #[id = "bypass"]
    bypass: BoolParam,
}

impl Default for TestParams {
    fn default() -> Self {
        Self {
            gain: FloatParam::new("Gain", 0.25, FloatRange::Linear { min: 0.0, max: 1.0 }),
            bypass: BoolParam::new("Bypass", false),
        }
    }
}

#[derive(Default)]
struct TestPlugin {
    params: Arc<TestParams>,
}

impl Plugin for TestPlugin {
    const NAME: &'static str = "Parameter test";
    const VENDOR: &'static str = "Test";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = "0.0.0";
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[];

    type Editor = ();
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn process(
        &mut self,
        _: &mut Buffer,
        _: &mut AuxiliaryBuffers,
        _: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        ProcessStatus::Normal
    }
}

impl Vst3Plugin for TestPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"ParameterTestVST";
    const VST3_SUBCATEGORIES: &'static [crate::wrapper::vst3::subcategories::Vst3SubCategory] = &[];
}

struct ReadbackHost {
    wrapper: Weak<Wrapper<TestPlugin>>,
    reads: Arc<Mutex<Vec<(u32, f64)>>>,
}

impl Class for ReadbackHost {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for ReadbackHost {
    unsafe fn beginEdit(&self, _: u32) -> tresult {
        kResultOk
    }

    unsafe fn performEdit(&self, _: u32, _: f64) -> tresult {
        kResultOk
    }

    unsafe fn endEdit(&self, id: u32) -> tresult {
        let wrapper = self.wrapper.upgrade().unwrap();
        let value = unsafe { wrapper.getParamNormalized(id) };
        self.reads.lock().unwrap().push((id, value));
        kResultOk
    }

    unsafe fn restartComponent(&self, _: i32) -> tresult {
        kResultOk
    }
}

fn fixture() -> (
    Arc<Wrapper<TestPlugin>>,
    Arc<TestParams>,
    Arc<Mutex<Vec<(u32, f64)>>>,
) {
    let wrapper = Arc::new(Wrapper::<TestPlugin>::new());
    let params = wrapper.inner.plugin.try_lock().unwrap().params.clone();
    let reads = Arc::new(Mutex::new(Vec::new()));
    let host = ComWrapper::new(ReadbackHost {
        wrapper: Arc::downgrade(&wrapper),
        reads: reads.clone(),
    });
    *wrapper.inner.component_handler.borrow_mut() = host.to_com_ptr::<IComponentHandler>();
    wrapper.inner.is_processing.store(true, Ordering::SeqCst);

    (wrapper, params, reads)
}

#[test]
fn single_edits_are_read_back_before_the_audio_callback() {
    let (wrapper, params, reads) = fixture();
    let gui = wrapper.inner.clone().make_gui_context();
    let setter = gui.param_setter();

    setter.begin_set_parameter(&params.bypass);
    setter.set_parameter(&params.bypass, true);
    setter.end_set_parameter(&params.bypass);

    setter.begin_set_parameter(&params.gain);
    setter.set_parameter(&params.gain, 0.8);
    setter.end_set_parameter(&params.gain);

    let gain_id = wrapper.inner.param_ptr_to_hash[&params.gain.as_ptr()];
    wrapper
        .inner
        .set_normalized_value_by_hash(gain_id, 0.8, None);

    setter.begin_set_parameter(&params.gain);
    setter.set_parameter(&params.gain, params.gain.default_plain_value());
    setter.end_set_parameter(&params.gain);

    let values: Vec<_> = reads
        .lock()
        .unwrap()
        .iter()
        .map(|&(_, value)| value as f32)
        .collect();
    assert_eq!(
        values,
        [1.0, 0.8, 0.25],
        "Host must see each completed edit, including reset"
    );
    assert!(
        !params.bypass.value(),
        "DSP parameters must wait for the audio callback"
    );
    assert_eq!(
        params.gain.value(),
        0.8,
        "Reset must also wait for the audio callback"
    );
}

#[test]
fn audio_acknowledgement_preserves_newer_edits_and_resumes_automation() {
    let (wrapper, params, _) = fixture();
    let gui = wrapper.inner.clone().make_gui_context();
    let setter = gui.param_setter();
    let id = wrapper.inner.param_ptr_to_hash[&params.gain.as_ptr()];

    setter.begin_set_parameter(&params.gain);
    setter.set_parameter(&params.gain, 0.8);
    setter.end_set_parameter(&params.gain);

    wrapper.inner.set_normalized_value_by_hash(id, 0.6, None);
    assert_eq!(unsafe { wrapper.getParamNormalized(id) } as f32, 0.8);
    assert_eq!(params.gain.value(), 0.6);

    wrapper.inner.set_normalized_value_by_hash(id, 0.8, None);
    assert_eq!(wrapper.inner.pending_controller_values[&id].load(), None);

    wrapper.inner.set_normalized_value_by_hash(id, 0.1, None);
    assert_eq!(unsafe { wrapper.getParamNormalized(id) } as f32, 0.1);

    assert_eq!(unsafe { wrapper.setParamNormalized(id, 0.9) }, kResultOk);
    assert_eq!(params.gain.value(), 0.1);
    assert_eq!(unsafe { wrapper.getParamNormalized(id) } as f32, 0.9);

    wrapper.inner.set_normalized_value_by_hash(id, 0.1, None);
    assert_eq!(unsafe { wrapper.getParamNormalized(id) } as f32, 0.9);

    wrapper.inner.set_normalized_value_by_hash(id, 0.9, None);
    assert_eq!(params.gain.value(), 0.9);
    assert_eq!(wrapper.inner.pending_controller_values[&id].load(), None);
}

#[test]
fn state_restore_discards_unconsumed_controller_edits() {
    let (wrapper, params, _) = fixture();
    let mut state = wrapper.inner.get_state_object();
    let gui = wrapper.inner.clone().make_gui_context();
    let setter = gui.param_setter();
    let id = wrapper.inner.param_ptr_to_hash[&params.bypass.as_ptr()];

    setter.begin_set_parameter(&params.bypass);
    setter.set_parameter(&params.bypass, true);
    setter.end_set_parameter(&params.bypass);
    assert_eq!(unsafe { wrapper.getParamNormalized(id) }, 1.0);

    assert!(wrapper.inner.set_state_inner(&mut state));
    assert_eq!(unsafe { wrapper.getParamNormalized(id) }, 0.0);
    assert!(!params.bypass.value());
}

#[test]
fn inactive_edits_update_processor_immediately() {
    let (wrapper, params, _) = fixture();
    wrapper.inner.is_processing.store(false, Ordering::SeqCst);
    let gui = wrapper.inner.clone().make_gui_context();
    let setter = gui.param_setter();
    let id = wrapper.inner.param_ptr_to_hash[&params.bypass.as_ptr()];

    setter.begin_set_parameter(&params.bypass);
    setter.set_parameter(&params.bypass, true);
    setter.end_set_parameter(&params.bypass);

    assert!(params.bypass.value());
    assert_eq!(unsafe { wrapper.getParamNormalized(id) }, 1.0);
    assert_eq!(wrapper.inner.pending_controller_values[&id].load(), None);
}
