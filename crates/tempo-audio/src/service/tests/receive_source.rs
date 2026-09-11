use super::*;
use crate::monitor::SpscRing;
use crate::receive_audio::{CaptureInput, ReceiveAudioFeed, ReceiveError, ReceiveOrigin};

struct CaptureBackend {
    ring: Arc<SpscRing>,
    media: Arc<ReceiveAudioFeed>,
    input: Option<CaptureInput>,
    releases: Arc<std::sync::atomic::AtomicUsize>,
}
impl AudioBackend for CaptureBackend {
    fn capture(&mut self) -> Vec<f32> {
        vec![0.1; 240]
    }
    fn play(&mut self, _samples: &[f32]) {
        panic!("receive source test must never transmit")
    }
    fn spectrum_tap(&self) -> Option<(Arc<SpscRing>, u32)> {
        Some((self.ring.clone(), 48_000))
    }
    fn capture_input(&self) -> Option<CaptureInput> {
        self.input.clone()
    }
    fn release_device(&mut self) {
        assert!(
            self.media.describe().is_none(),
            "media must end before device teardown starts"
        );
        self.releases
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.input = None;
    }
}

#[test]
fn audio_rebuild_binds_the_opened_source_and_retires_old_media_before_both_success_and_failure() {
    for succeeds in [false, true] {
        let engine = Arc::new(Mutex::new(Engine::new("N2SOURCE", "FN31", 0)));
        let mut initial = engine_lock(&engine).settings().clone();
        initial.ptt_method = "vox".into();
        initial.rig_model = 0;
        initial.audio_in = "Requested codec".into();
        engine_lock(&engine).apply_settings(initial);
        let mut state = loop_state();
        state.applied = Transport::from_settings(engine_lock(&engine).settings());
        let radio_id = engine_lock(&engine).settings().active_radio;
        state.remote_radio_id = Some(radio_id);
        let media = state.rx_tap.receive_audio();
        let ring = Arc::new(SpscRing::new(4096));
        let input = CaptureInput {
            device: "Previous codec".into(),
            system_default: false,
        };
        let origin =
            ReceiveOrigin::from_open(Some(radio_id), "Requested codec", Some(input.clone()));
        state
            .rx_tap
            .publish_card_with_origin(ring.clone(), 48_000, origin);
        let old = media.subscribe(media.source().unwrap()).unwrap();
        let releases = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut backend = CaptureBackend {
            ring,
            media: media.clone(),
            input: Some(input),
            releases: releases.clone(),
        };
        let mut rig = Rig::vox();
        let mut sinks = StationSinks::new();
        let mut rr = mock_reopen_rig();
        let opens = std::cell::Cell::new(0);
        let mut reopen = |want: &Transport| {
            opens.set(opens.get() + 1);
            assert!(media.source().is_none());
            assert!(matches!(old.read(Instant::now()), Err(ReceiveError::Ended)));
            assert_eq!(want.audio_in, "Requested codec");
            // A user can change Settings while a slow OS open is in progress.
            // Its later selection cannot label the source this open returns.
            let mut latest = engine_lock(&engine).settings().clone();
            latest.audio_in = "Later selection".into();
            engine_lock(&engine).apply_settings(latest);
            if !succeeds {
                return Err("synthetic open refusal".into());
            }
            Ok(CaptureBackend {
                ring: Arc::new(SpscRing::new(4096)),
                media: media.clone(),
                input: Some(CaptureInput {
                    device: "Resolved opened codec".into(),
                    system_default: false,
                }),
                releases: releases.clone(),
            })
        };
        state.force_audio_rebuild = true;
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &no_sinks(),
                0.0,
                &mut reopen,
                &mut rr,
                &mut sinks,
            )
            .unwrap();
        assert_eq!(
            opens.get(),
            1,
            "positive control reaches the actual reopen closure"
        );
        assert_eq!(releases.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(matches!(old.read(Instant::now()), Err(ReceiveError::Ended)));
        if succeeds {
            let description = media.describe().unwrap();
            let origin = description.origin.unwrap();
            assert_eq!(origin.radio_id, radio_id);
            assert_eq!(origin.requested_input, "Requested codec");
            assert_eq!(origin.input.device, "Resolved opened codec");
            assert!(!origin.input.system_default);
            assert_eq!(engine_lock(&engine).settings().audio_in, "Later selection");
            let reader = media.subscribe(description.source).unwrap();
            drop(old);
            assert!(reader.read(Instant::now()).is_ok());
        } else {
            assert!(media.describe().is_none());
            assert!(
                state.audio_retry_at.is_some(),
                "local recovery remains scheduled"
            );
        }
        assert!(!engine_lock(&engine).tx_enabled());
    }
}
