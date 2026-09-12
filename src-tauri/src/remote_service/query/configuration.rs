//! Explicit configuration reads for the existing Nexus settings and programming
//! surfaces. Omitted fields are never serialized, read from the vault, defaulted
//! into an apparent station choice, or admitted as write-back settings.
use super::{navigation, Collection};
use serde::{ser::SerializeStruct, Serialize, Serializer};
use serde_json::{json, Value};
use std::{io::Read, path::Path};
use tempo_app::settings::{RadioProfile, Settings};

pub(super) fn build(kind: Collection, engine: &crate::SharedEngine) -> Result<Value, &'static str> {
    match kind {
        Collection::Settings => {
            let e = engine.try_lock().map_err(|_| "applicationBusy")?;
            settings(e.settings())
        }
        Collection::Programming => {
            let grid = engine
                .try_lock()
                .map_err(|_| "applicationBusy")?
                .settings()
                .mygrid
                .clone();
            programming(&crate::radioprog_path(), &grid)
        }
        _ => Err("applicationUnsupported"),
    }
}

// Explicit borrowed fields make a future native credential field private by
// default. The schema-coverage test requires reviewing every new field.
struct SettingsView<'a>(&'a Settings);
struct RadioView<'a>(&'a RadioProfile);

#[cfg(test)]
pub(super) const SETTINGS_KEYS: &[&str] = &[
    // The contest station data and the CW-reverse preference are OPERATING
    // parameters, not credentials: a remote operator needs to see the exchange
    // their station is sending. WITHHELD_KEYS is service identities and keys.
    "activeRadio",
    "aiCwEnabled",
    "alertConfirmTier",
    "alertCq",
    "alertDxccBands",
    "alertGridBands",
    "alertMyCall",
    "alertNew",
    "alertRareGridBands",
    "ampFollowBand",
    "ampModel",
    "ampPort",
    "announceVerbosity",
    "antRxGainDbi",
    "antTxGainDbi",
    "apCqOnly",
    "apDecode",
    "aprsChannelMhz",
    "aprsComment",
    "aprsIsEnabled",
    "aprsIsHost",
    "aprsIsMessages",
    "aprsIsObjects",
    "aprsIsPort",
    "aprsIsRadiusKm",
    "aprsIsUplink",
    "aprsIsWatchCalls",
    "aprsIsWeather",
    "aprsPath",
    "aprsSsid",
    "aprsStationTtlMin",
    "aprsSymbolCode",
    "aprsSymbolTable",
    "audioIn",
    "audioOut",
    "autoLog",
    "b4MatchMode",
    "band",
    "bandEdgeTones",
    "baud",
    "beacon",
    "beaconPowerDbm",
    "beaconRrSlot",
    "beaconRrSlots",
    "beaconTxPercent",
    "bestCaller",
    "bestCallerMinSnr",
    "betaUpdates",
    "blockedCalls",
    "catBroker",
    "catBrokerPort",
    "catBrokerPtt",
    "catDtrState",
    "catPttLineState",
    "catRtsKeysPtt",
    "catRtsState",
    "catSerialHandshake",
    "chatImplicitAck",
    "chatMaxCycles",
    "clearDxAfterLog",
    "clockCheck",
    "cloudlogUpload",
    "clublogUpload",
    "clusterEnabled",
    "clusterHost",
    "clusterHosts",
    "companionAddr",
    "connectWeb",
    "connectWebPort",
    "contestCategoryAssisted",
    "contestCategoryOperator",
    "contestCategoryPower",
    "contestCategoryStation",
    "contestCheck",
    "contestCqZone",
    "contestItuZone",
    "contestPower",
    "contestQthCounty",
    "contestQthState",
    "cqMaxCalls",
    "cqPauseSecs",
    "cqStallOvers",
    "ctcssToneHz",
    "cwIdAfter73",
    "cwKeyLine",
    "cwKeyPort",
    "cwKeyer",
    "cwPitchHz",
    "cwReverse",
    "cwWpm",
    "dataModesPlainSsb",
    "decodeDepth",
    "decodeFHighHz",
    "decodeFLowHz",
    "defaultRadio",
    "diagDebugLog",
    "dialMhz",
    "directedMaxCalls",
    "disableTxAfter73",
    "doubleClickSetsTx",
    "dxkeeperBasePort",
    "dxkeeperHost",
    "dxkeeperUploads",
    "eqslUpload",
    "fdActive",
    "fdBonuses",
    "fdBonusesPlanned",
    "fdClass",
    "fdEvent",
    "fdEventName",
    "fdHostEnable",
    "fdHostPort",
    "fdJoinAddr",
    "fdOperator",
    "fdPositionId",
    "fdPositionName",
    "fdPowerMult",
    "fdScoreboard",
    "fdScoreboardPort",
    "fdSection",
    "flexNativeAudio",
    "flexNativePan",
    "flexRadioIp",
    "fst4PeriodS",
    "harqEnabled",
    "holdTxFreq",
    "hrdLogging",
    "hrdUdpAddr",
    "hrdlogUpload",
    "icomDataMode",
    "icomNativeCat",
    "issSstvAutoArm",
    "journeyStreakEnabled",
    "js8Autoreply",
    "js8CqIntervalMin",
    "js8Groups",
    "js8HbAck",
    "js8HbIntervalMin",
    "js8IdleWatchdogMin",
    "js8Info",
    "js8Relay",
    "js8RxSpeeds",
    "js8Speed",
    "js8Status",
    "jt65Submode",
    "licenseClass",
    "logReportsToComments",
    "lotwAutoUpload",
    "lotwAutoUploadHours",
    "lotwLastAutoUploadUnix",
    "lotwMaxAgeDays",
    "lotwUseAdifLocation",
    "macros",
    "maxPowerAm",
    "maxPowerCw",
    "maxPowerDigital",
    "maxPowerPhone",
    "monitorDevice",
    "monitorEnabled",
    "monitorLevel",
    "msk144PeriodS",
    "mycall",
    "mygrid",
    "n1mmAddr",
    "n1mmUpload",
    "n3fjpHost",
    "n3fjpPort",
    "n3fjpReportBand",
    "n3fjpUpload",
    "n3fjpUseEnter",
    "omnirigSlot",
    "opName",
    "opState",
    "openingRegional",
    "operatingMode",
    "phoneMode",
    "potaNewActivationAlert",
    "pounceThreshold",
    "preferRrr",
    "promptToLog",
    "propEngine",
    "pskRxAutoArm",
    "pskreporter",
    "pttMethod",
    "pttSerialPort",
    "q65PeriodS",
    "q65Submode",
    "qrzAutoSync",
    "qrzLastSyncUnix",
    "qrzLogbookUpload",
    "qrzSyncHours",
    "qsyCadence",
    "qsyEnabled",
    "qsySet",
    "radioPegged",
    "radios",
    "rigAddr",
    "rigConn",
    "rigModel",
    "rigModelName",
    "rigctldPort",
    "rotAllowFlip",
    "rotCalAzDeg",
    "rotCalElDeg",
    "rotParkAz",
    "rotParkEl",
    "rotPostPass",
    "rotReadyAz",
    "rotReadyEl",
    "rotTolAzDeg",
    "rotTolElDeg",
    "rotatorBaud",
    "rotatorHost",
    "rotatorModel",
    "rotatorPort",
    "routingRules",
    "rptrOffsetOverrideHz",
    "rptrShift",
    "rttyBackend",
    "rttyBaud",
    "rttyFskLine",
    "rttyFskPort",
    "rttyReverse",
    "rttyRxAutoArm",
    "rttyShiftHz",
    "rxGain",
    "rxOffsetHz",
    "satDopplerOff",
    "satMinShiftHz",
    "satPassAlertSoundOff",
    "satUpdateMs",
    "satUplinkRadios",
    "satVfoMap",
    "saveQsoWav",
    "saveWav",
    "serialPort",
    "setRigMode",
    "sideband",
    "simultaneousRadios",
    "singleDecode",
    "soundDecodeTick",
    "soundTxState",
    "source",
    "specialOp",
    "splitDetectEnabled",
    "splitMode",
    "sstvDefaultTxMode",
    "sstvHoldDataSubmode",
    "sstvRxAutoArm",
    "sstvTxPowerPct",
    "stationPowerW",
    "tunePowerPct",
    "tuneTimeoutSecs",
    "txEven",
    "txLevel",
    "txOffsetHz",
    "txWatchdogMin",
    "unassistedMode",
    "units",
    "voiceMicDevice",
    "wantedCalls",
    "wheelTuneSensitivity",
    "winkeyerPort",
    "workingFrequencies",
    "writeAllTxt",
    "wrlUpload",
    "wsjtxUdp",
    "wsjtxUdpAddr",
    "yaesuRfScope",
];
pub(super) const WITHHELD_KEYS: &[&str] = &[
    "lotwUsername",
    "lotwLastQsl",
    "lotwStationLocation",
    "tqslPath",
    "eqslUsername",
    "eqslLastSync",
    "qrzUsername",
    "hamqthUsername",
    "clublogEmail",
    "clublogCallsign",
    "clublogApiKey",
    "eqslQthNickname",
    "wrlLogbookId",
    "cloudlogUrl",
    "cloudlogStationId",
    "cloudlogKey",
    "voiceMessages",
];
impl Serialize for SettingsView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("SettingsView", 269)?;
        out.serialize_field("mycall", &self.0.mycall)?;
        out.serialize_field("mygrid", &self.0.mygrid)?;
        out.serialize_field("opName", &self.0.op_name)?;
        out.serialize_field("opState", &self.0.op_state)?;
        out.serialize_field("catBrokerPtt", &self.0.cat_broker_ptt)?;
        out.serialize_field("band", &self.0.band)?;
        out.serialize_field("dialMhz", &self.0.dial_mhz)?;
        out.serialize_field("sideband", &self.0.sideband)?;
        out.serialize_field("phoneMode", &self.0.phone_mode)?;
        out.serialize_field("rptrShift", &self.0.rptr_shift)?;
        out.serialize_field("ctcssToneHz", &self.0.ctcss_tone_hz)?;
        out.serialize_field("q65PeriodS", &self.0.q65_period_s)?;
        out.serialize_field("beaconTxPercent", &self.0.beacon_tx_percent)?;
        out.serialize_field("beaconPowerDbm", &self.0.beacon_power_dbm)?;
        out.serialize_field("beaconRrSlot", &self.0.beacon_rr_slot)?;
        out.serialize_field("beaconRrSlots", &self.0.beacon_rr_slots)?;
        out.serialize_field("fst4PeriodS", &self.0.fst4_period_s)?;
        out.serialize_field("msk144PeriodS", &self.0.msk144_period_s)?;
        out.serialize_field("jt65Submode", &self.0.jt65_submode)?;
        out.serialize_field("q65Submode", &self.0.q65_submode)?;
        out.serialize_field("js8Speed", &self.0.js8_speed)?;
        out.serialize_field("js8RxSpeeds", &self.0.js8_rx_speeds)?;
        out.serialize_field("js8HbIntervalMin", &self.0.js8_hb_interval_min)?;
        out.serialize_field("js8CqIntervalMin", &self.0.js8_cq_interval_min)?;
        out.serialize_field("js8HbAck", &self.0.js8_hb_ack)?;
        out.serialize_field("js8Autoreply", &self.0.js8_autoreply)?;
        out.serialize_field("js8Relay", &self.0.js8_relay)?;
        out.serialize_field("js8IdleWatchdogMin", &self.0.js8_idle_watchdog_min)?;
        out.serialize_field("js8Info", &self.0.js8_info)?;
        out.serialize_field("js8Status", &self.0.js8_status)?;
        out.serialize_field("js8Groups", &self.0.js8_groups)?;
        out.serialize_field("rptrOffsetOverrideHz", &self.0.rptr_offset_override_hz)?;
        out.serialize_field("fdActive", &self.0.fd_active)?;
        out.serialize_field("fdClass", &self.0.fd_class)?;
        // The contest station data: what this station SENDS as its exchange. Beside
        // fdClass/fdSection because it is the same kind of thing — a remote operator
        // needs to see the exchange going out under their hand.
        out.serialize_field("contestQthCounty", &self.0.contest_qth_county)?;
        out.serialize_field("contestQthState", &self.0.contest_qth_state)?;
        out.serialize_field("contestCheck", &self.0.contest_check)?;
        out.serialize_field("contestCqZone", &self.0.contest_cq_zone)?;
        out.serialize_field("contestItuZone", &self.0.contest_itu_zone)?;
        out.serialize_field("contestPower", &self.0.contest_power)?;
        out.serialize_field("contestCategoryOperator", &self.0.contest_category_operator)?;
        out.serialize_field("contestCategoryPower", &self.0.contest_category_power)?;
        out.serialize_field("contestCategoryAssisted", &self.0.contest_category_assisted)?;
        out.serialize_field("contestCategoryStation", &self.0.contest_category_station)?;
        // Which side of the BFO CW sits on — a station preference, not a credential.
        out.serialize_field("cwReverse", &self.0.cw_reverse)?;
        out.serialize_field("fdEvent", &self.0.fd_event)?;
        out.serialize_field("fdPowerMult", &self.0.fd_power_mult)?;
        out.serialize_field("fdBonuses", &self.0.fd_bonuses)?;
        out.serialize_field("fdBonusesPlanned", &self.0.fd_bonuses_planned)?;
        out.serialize_field("n3fjpHost", &self.0.n3fjp_host)?;
        out.serialize_field("n3fjpPort", &self.0.n3fjp_port)?;
        out.serialize_field("n3fjpUseEnter", &self.0.n3fjp_use_enter)?;
        out.serialize_field("n3fjpReportBand", &self.0.n3fjp_report_band)?;
        out.serialize_field("dxkeeperHost", &self.0.dxkeeper_host)?;
        out.serialize_field("dxkeeperBasePort", &self.0.dxkeeper_base_port)?;
        out.serialize_field("dxkeeperUploads", &self.0.dxkeeper_uploads)?;
        out.serialize_field("n1mmAddr", &self.0.n1mm_addr)?;
        out.serialize_field("n1mmUpload", &self.0.n1mm_upload)?;
        out.serialize_field("fdSection", &self.0.fd_section)?;
        out.serialize_field("fdOperator", &self.0.fd_operator)?;
        out.serialize_field("fdHostEnable", &self.0.fd_host_enable)?;
        out.serialize_field("fdHostPort", &self.0.fd_host_port)?;
        out.serialize_field("fdEventName", &self.0.fd_event_name)?;
        out.serialize_field("fdJoinAddr", &self.0.fd_join_addr)?;
        out.serialize_field("fdPositionName", &self.0.fd_position_name)?;
        out.serialize_field("fdPositionId", &self.0.fd_position_id)?;
        out.serialize_field("fdScoreboard", &self.0.fd_scoreboard)?;
        out.serialize_field("fdScoreboardPort", &self.0.fd_scoreboard_port)?;
        out.serialize_field("connectWeb", &self.0.connect_web)?;
        out.serialize_field("connectWebPort", &self.0.connect_web_port)?;
        out.serialize_field("betaUpdates", &self.0.beta_updates)?;
        out.serialize_field("beacon", &self.0.beacon)?;
        out.serialize_field("harqEnabled", &self.0.harq_enabled)?;
        out.serialize_field("pttMethod", &self.0.ptt_method)?;
        out.serialize_field("rigModel", &self.0.rig_model)?;
        out.serialize_field("rigModelName", &self.0.rig_model_name)?;
        out.serialize_field("serialPort", &self.0.serial_port)?;
        out.serialize_field("pttSerialPort", &self.0.ptt_serial_port)?;
        out.serialize_field("catRtsState", &self.0.cat_rts_state)?;
        out.serialize_field("catDtrState", &self.0.cat_dtr_state)?;
        out.serialize_field("catRtsKeysPtt", &self.0.cat_rts_keys_ptt)?;
        out.serialize_field("catSerialHandshake", &self.0.cat_serial_handshake)?;
        out.serialize_field("catPttLineState", &self.0.cat_ptt_line_state)?;
        out.serialize_field("baud", &self.0.baud)?;
        out.serialize_field("rigConn", &self.0.rig_conn)?;
        out.serialize_field("rigAddr", &self.0.rig_addr)?;
        out.serialize_field("omnirigSlot", &self.0.omnirig_slot)?;
        out.serialize_field("icomNativeCat", &self.0.icom_native_cat)?;
        out.serialize_field("splitDetectEnabled", &self.0.split_detect_enabled)?;
        out.serialize_field("yaesuRfScope", &self.0.yaesu_rf_scope)?;
        out.serialize_field("icomDataMode", &self.0.icom_data_mode)?;
        out.serialize_field("dataModesPlainSsb", &self.0.data_modes_plain_ssb)?;
        out.serialize_field("sstvHoldDataSubmode", &self.0.sstv_hold_data_submode)?;
        out.serialize_field("setRigMode", &self.0.set_rig_mode)?;
        out.serialize_field("operatingMode", &self.0.operating_mode)?;
        out.serialize_field("licenseClass", &self.0.license_class)?;
        out.serialize_field("cwKeyer", &self.0.cw_keyer)?;
        out.serialize_field("winkeyerPort", &self.0.winkeyer_port)?;
        out.serialize_field("cwKeyPort", &self.0.cw_key_port)?;
        out.serialize_field("cwKeyLine", &self.0.cw_key_line)?;
        out.serialize_field("cwPitchHz", &self.0.cw_pitch_hz)?;
        out.serialize_field("cwWpm", &self.0.cw_wpm)?;
        out.serialize_field("rttyBackend", &self.0.rtty_backend)?;
        out.serialize_field("rttyFskLine", &self.0.rtty_fsk_line)?;
        out.serialize_field("rttyFskPort", &self.0.rtty_fsk_port)?;
        out.serialize_field("rttyBaud", &self.0.rtty_baud)?;
        out.serialize_field("rttyShiftHz", &self.0.rtty_shift_hz)?;
        out.serialize_field("rttyReverse", &self.0.rtty_reverse)?;
        out.serialize_field("aiCwEnabled", &self.0.ai_cw_enabled)?;
        out.serialize_field("unassistedMode", &self.0.unassisted_mode)?;
        out.serialize_field("rigctldPort", &self.0.rigctld_port)?;
        out.serialize_field("rotatorModel", &self.0.rotator_model)?;
        out.serialize_field("rotatorPort", &self.0.rotator_port)?;
        out.serialize_field("rotatorBaud", &self.0.rotator_baud)?;
        out.serialize_field("ampModel", &self.0.amp_model)?;
        out.serialize_field("ampPort", &self.0.amp_port)?;
        out.serialize_field("ampFollowBand", &self.0.amp_follow_band)?;
        out.serialize_field("rotatorHost", &self.0.rotator_host)?;
        out.serialize_field("rotParkAz", &self.0.rot_park_az)?;
        out.serialize_field("rotParkEl", &self.0.rot_park_el)?;
        out.serialize_field("rotReadyAz", &self.0.rot_ready_az)?;
        out.serialize_field("rotReadyEl", &self.0.rot_ready_el)?;
        out.serialize_field("rotPostPass", &self.0.rot_post_pass)?;
        out.serialize_field("rotTolAzDeg", &self.0.rot_tol_az_deg)?;
        out.serialize_field("rotTolElDeg", &self.0.rot_tol_el_deg)?;
        out.serialize_field("rotCalAzDeg", &self.0.rot_cal_az_deg)?;
        out.serialize_field("rotCalElDeg", &self.0.rot_cal_el_deg)?;
        out.serialize_field("rotAllowFlip", &self.0.rot_allow_flip)?;
        out.serialize_field("satDopplerOff", &self.0.sat_doppler_off)?;
        out.serialize_field("satVfoMap", &self.0.sat_vfo_map)?;
        out.serialize_field("satUplinkRadios", &self.0.sat_uplink_radios)?;
        out.serialize_field("satMinShiftHz", &self.0.sat_min_shift_hz)?;
        out.serialize_field("satUpdateMs", &self.0.sat_update_ms)?;
        out.serialize_field("satPassAlertSoundOff", &self.0.sat_pass_alert_sound_off)?;
        out.serialize_field("catBroker", &self.0.cat_broker)?;
        out.serialize_field("catBrokerPort", &self.0.cat_broker_port)?;
        out.serialize_field("flexRadioIp", &self.0.flex_radio_ip)?;
        out.serialize_field("flexNativePan", &self.0.flex_native_pan)?;
        out.serialize_field("flexNativeAudio", &self.0.flex_native_audio)?;
        out.serialize_field(
            "radios",
            &self.0.radios.iter().map(RadioView).collect::<Vec<_>>(),
        )?;
        out.serialize_field("activeRadio", &self.0.active_radio)?;
        out.serialize_field("radioPegged", &self.0.radio_pegged)?;
        out.serialize_field("simultaneousRadios", &self.0.simultaneous_radios)?;
        out.serialize_field("routingRules", &self.0.routing_rules)?;
        out.serialize_field("defaultRadio", &self.0.default_radio)?;
        out.serialize_field("wsjtxUdp", &self.0.wsjtx_udp)?;
        out.serialize_field("wsjtxUdpAddr", &self.0.wsjtx_udp_addr)?;
        out.serialize_field("writeAllTxt", &self.0.write_all_txt)?;
        out.serialize_field("diagDebugLog", &self.0.diag_debug_log)?;
        out.serialize_field("hrdLogging", &self.0.hrd_logging)?;
        out.serialize_field("hrdUdpAddr", &self.0.hrd_udp_addr)?;
        out.serialize_field("companionAddr", &self.0.companion_addr)?;
        out.serialize_field("source", &self.0.source)?;
        out.serialize_field("pskreporter", &self.0.pskreporter)?;
        out.serialize_field("clusterEnabled", &self.0.cluster_enabled)?;
        out.serialize_field("clusterHost", &self.0.cluster_host)?;
        out.serialize_field("clusterHosts", &self.0.cluster_hosts)?;
        out.serialize_field("aprsIsEnabled", &self.0.aprs_is_enabled)?;
        out.serialize_field("aprsIsHost", &self.0.aprs_is_host)?;
        out.serialize_field("aprsIsPort", &self.0.aprs_is_port)?;
        out.serialize_field("aprsIsRadiusKm", &self.0.aprs_is_radius_km)?;
        out.serialize_field("aprsIsWatchCalls", &self.0.aprs_is_watch_calls)?;
        out.serialize_field("aprsIsWeather", &self.0.aprs_is_weather)?;
        out.serialize_field("aprsIsObjects", &self.0.aprs_is_objects)?;
        out.serialize_field("aprsIsMessages", &self.0.aprs_is_messages)?;
        out.serialize_field("aprsIsUplink", &self.0.aprs_is_uplink)?;
        out.serialize_field("aprsStationTtlMin", &self.0.aprs_station_ttl_min)?;
        out.serialize_field("aprsChannelMhz", &self.0.aprs_channel_mhz)?;
        out.serialize_field("aprsSymbolCode", &self.0.aprs_symbol_code)?;
        out.serialize_field("aprsSymbolTable", &self.0.aprs_symbol_table)?;
        out.serialize_field("aprsComment", &self.0.aprs_comment)?;
        out.serialize_field("aprsPath", &self.0.aprs_path)?;
        out.serialize_field("aprsSsid", &self.0.aprs_ssid)?;
        out.serialize_field("audioIn", &self.0.audio_in)?;
        out.serialize_field("audioOut", &self.0.audio_out)?;
        out.serialize_field("voiceMicDevice", &self.0.voice_mic_device)?;
        out.serialize_field("txLevel", &self.0.tx_level)?;
        out.serialize_field("rxGain", &self.0.rx_gain)?;
        out.serialize_field("monitorEnabled", &self.0.monitor_enabled)?;
        out.serialize_field("monitorDevice", &self.0.monitor_device)?;
        out.serialize_field("monitorLevel", &self.0.monitor_level)?;
        out.serialize_field("stationPowerW", &self.0.station_power_w)?;
        out.serialize_field("units", &self.0.units)?;
        out.serialize_field("maxPowerPhone", &self.0.max_power_phone)?;
        out.serialize_field("maxPowerCw", &self.0.max_power_cw)?;
        out.serialize_field("maxPowerDigital", &self.0.max_power_digital)?;
        out.serialize_field("maxPowerAm", &self.0.max_power_am)?;
        out.serialize_field("propEngine", &self.0.prop_engine)?;
        out.serialize_field("saveWav", &self.0.save_wav)?;
        out.serialize_field("lotwMaxAgeDays", &self.0.lotw_max_age_days)?;
        out.serialize_field("antTxGainDbi", &self.0.ant_tx_gain_dbi)?;
        out.serialize_field("antRxGainDbi", &self.0.ant_rx_gain_dbi)?;
        out.serialize_field("journeyStreakEnabled", &self.0.journey_streak_enabled)?;
        out.serialize_field("txWatchdogMin", &self.0.tx_watchdog_min)?;
        out.serialize_field("txEven", &self.0.tx_even)?;
        out.serialize_field("rxOffsetHz", &self.0.rx_offset_hz)?;
        out.serialize_field("txOffsetHz", &self.0.tx_offset_hz)?;
        out.serialize_field("holdTxFreq", &self.0.hold_tx_freq)?;
        out.serialize_field("clockCheck", &self.0.clock_check)?;
        out.serialize_field("autoLog", &self.0.auto_log)?;
        out.serialize_field("promptToLog", &self.0.prompt_to_log)?;
        out.serialize_field("saveQsoWav", &self.0.save_qso_wav)?;
        out.serialize_field("preferRrr", &self.0.prefer_rrr)?;
        out.serialize_field("cqMaxCalls", &self.0.cq_max_calls)?;
        out.serialize_field("cqPauseSecs", &self.0.cq_pause_secs)?;
        out.serialize_field("directedMaxCalls", &self.0.directed_max_calls)?;
        out.serialize_field("chatMaxCycles", &self.0.chat_max_cycles)?;
        out.serialize_field("chatImplicitAck", &self.0.chat_implicit_ack)?;
        out.serialize_field("cqStallOvers", &self.0.cq_stall_overs)?;
        out.serialize_field("disableTxAfter73", &self.0.disable_tx_after_73)?;
        out.serialize_field("bandEdgeTones", &self.0.band_edge_tones)?;
        out.serialize_field("cwIdAfter73", &self.0.cw_id_after_73)?;
        out.serialize_field("clearDxAfterLog", &self.0.clear_dx_after_log)?;
        out.serialize_field("doubleClickSetsTx", &self.0.double_click_sets_tx)?;
        out.serialize_field("tuneTimeoutSecs", &self.0.tune_timeout_secs)?;
        out.serialize_field("tunePowerPct", &self.0.tune_power_pct)?;
        out.serialize_field("splitMode", &self.0.split_mode)?;
        out.serialize_field("decodeDepth", &self.0.decode_depth)?;
        out.serialize_field("decodeFLowHz", &self.0.decode_flow_hz)?;
        out.serialize_field("decodeFHighHz", &self.0.decode_fhigh_hz)?;
        out.serialize_field("apDecode", &self.0.ap_decode)?;
        out.serialize_field("apCqOnly", &self.0.ap_cq_only)?;
        out.serialize_field("singleDecode", &self.0.single_decode)?;
        out.serialize_field("specialOp", &self.0.special_op)?;
        out.serialize_field("workingFrequencies", &self.0.working_frequencies)?;
        out.serialize_field("qsyEnabled", &self.0.qsy_enabled)?;
        out.serialize_field("qsySet", &self.0.qsy_set)?;
        out.serialize_field("qsyCadence", &self.0.qsy_cadence)?;
        out.serialize_field("issSstvAutoArm", &self.0.iss_sstv_auto_arm)?;
        out.serialize_field("sstvRxAutoArm", &self.0.sstv_rx_auto_arm)?;
        out.serialize_field("sstvDefaultTxMode", &self.0.sstv_default_tx_mode)?;
        out.serialize_field("sstvTxPowerPct", &self.0.sstv_tx_power_pct)?;
        out.serialize_field("pskRxAutoArm", &self.0.psk_rx_auto_arm)?;
        out.serialize_field("rttyRxAutoArm", &self.0.rtty_rx_auto_arm)?;
        out.serialize_field("alertMyCall", &self.0.alert_my_call)?;
        out.serialize_field("alertCq", &self.0.alert_cq)?;
        out.serialize_field("alertNew", &self.0.alert_new)?;
        out.serialize_field("potaNewActivationAlert", &self.0.pota_new_activation_alert)?;
        out.serialize_field("logReportsToComments", &self.0.log_reports_to_comments)?;
        out.serialize_field("alertConfirmTier", &self.0.alert_confirm_tier)?;
        out.serialize_field("alertDxccBands", &self.0.alert_dxcc_bands)?;
        out.serialize_field("alertGridBands", &self.0.alert_grid_bands)?;
        out.serialize_field("b4MatchMode", &self.0.b4_match_mode)?;
        out.serialize_field("alertRareGridBands", &self.0.alert_rare_grid_bands)?;
        out.serialize_field("wheelTuneSensitivity", &self.0.wheel_tune_sensitivity)?;
        out.serialize_field("announceVerbosity", &self.0.announce_verbosity)?;
        out.serialize_field("soundTxState", &self.0.sound_tx_state)?;
        out.serialize_field("soundDecodeTick", &self.0.sound_decode_tick)?;
        out.serialize_field("bestCaller", &self.0.best_caller)?;
        out.serialize_field("bestCallerMinSnr", &self.0.best_caller_min_snr)?;
        out.serialize_field("blockedCalls", &self.0.blocked_calls)?;
        out.serialize_field("wantedCalls", &self.0.wanted_calls)?;
        out.serialize_field("pounceThreshold", &self.0.pounce_threshold)?;
        out.serialize_field("lotwUseAdifLocation", &self.0.lotw_use_adif_location)?;
        out.serialize_field("lotwAutoUpload", &self.0.lotw_auto_upload)?;
        out.serialize_field("lotwAutoUploadHours", &self.0.lotw_auto_upload_hours)?;
        out.serialize_field("lotwLastAutoUploadUnix", &self.0.lotw_last_auto_upload_unix)?;
        out.serialize_field("qrzLogbookUpload", &self.0.qrz_logbook_upload)?;
        out.serialize_field("qrzAutoSync", &self.0.qrz_auto_sync)?;
        out.serialize_field("qrzSyncHours", &self.0.qrz_sync_hours)?;
        out.serialize_field("qrzLastSyncUnix", &self.0.qrz_last_sync_unix)?;
        out.serialize_field("clublogUpload", &self.0.clublog_upload)?;
        out.serialize_field("eqslUpload", &self.0.eqsl_upload)?;
        out.serialize_field("hrdlogUpload", &self.0.hrdlog_upload)?;
        out.serialize_field("wrlUpload", &self.0.wrl_upload)?;
        out.serialize_field("n3fjpUpload", &self.0.n3fjp_upload)?;
        out.serialize_field("cloudlogUpload", &self.0.cloudlog_upload)?;
        out.serialize_field("openingRegional", &self.0.opening_regional)?;
        out.serialize_field("macros", &self.0.macros)?;
        out.end()
    }
}
impl Serialize for RadioView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("RadioView", 39)?;
        out.serialize_field("id", &self.0.id)?;
        out.serialize_field("name", &self.0.name)?;
        out.serialize_field("enabled", &self.0.enabled)?;
        out.serialize_field("pttMethod", &self.0.ptt_method)?;
        out.serialize_field("rigModel", &self.0.rig_model)?;
        out.serialize_field("rigModelName", &self.0.rig_model_name)?;
        out.serialize_field("serialPort", &self.0.serial_port)?;
        out.serialize_field("pttSerialPort", &self.0.ptt_serial_port)?;
        out.serialize_field("baud", &self.0.baud)?;
        out.serialize_field("rigConn", &self.0.rig_conn)?;
        out.serialize_field("rigAddr", &self.0.rig_addr)?;
        out.serialize_field("omnirigSlot", &self.0.omnirig_slot)?;
        out.serialize_field("rigctldPort", &self.0.rigctld_port)?;
        out.serialize_field("icomDataMode", &self.0.icom_data_mode)?;
        out.serialize_field("icomNativeCat", &self.0.icom_native_cat)?;
        out.serialize_field("dataModesPlainSsb", &self.0.data_modes_plain_ssb)?;
        out.serialize_field("sstvHoldDataSubmode", &self.0.sstv_hold_data_submode)?;
        out.serialize_field("audioIn", &self.0.audio_in)?;
        out.serialize_field("audioOut", &self.0.audio_out)?;
        out.serialize_field("txLevel", &self.0.tx_level)?;
        out.serialize_field("rxGain", &self.0.rx_gain)?;
        out.serialize_field("rotatorModel", &self.0.rotator_model)?;
        out.serialize_field("rotatorPort", &self.0.rotator_port)?;
        out.serialize_field("rotatorBaud", &self.0.rotator_baud)?;
        out.serialize_field("ampModel", &self.0.amp_model)?;
        out.serialize_field("ampPort", &self.0.amp_port)?;
        out.serialize_field("ampFollowBand", &self.0.amp_follow_band)?;
        out.serialize_field("rotatorHost", &self.0.rotator_host)?;
        out.serialize_field("rotctldPort", &self.0.rotctld_port)?;
        out.serialize_field("bands", &self.0.bands)?;
        out.serialize_field("lastDialMhz", &self.0.last_dial_mhz)?;
        out.serialize_field("lastBand", &self.0.last_band)?;
        out.serialize_field("lastSideband", &self.0.last_sideband)?;
        out.serialize_field("nativeScope", &self.0.native_scope)?;
        out.serialize_field("flexRadioIp", &self.0.flex_radio_ip)?;
        out.serialize_field("flexNativePan", &self.0.flex_native_pan)?;
        out.serialize_field("yaesuRfScope", &self.0.yaesu_rf_scope)?;
        out.serialize_field("yaesuFixStarts", &self.0.yaesu_fix_starts)?;
        out.serialize_field("flexNativeAudio", &self.0.flex_native_audio)?;
        out.end()
    }
}

fn revision(value: &Value) -> Result<String, &'static str> {
    let bytes = navigation::bounded(value)?;
    Ok(ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn settings_values(s: &Settings) -> Result<Value, &'static str> {
    if s.radios.len() > 64 {
        return Err("applicationTooLarge");
    }
    navigation::value(&SettingsView(s))
}
pub(super) fn settings_revision(s: &Settings) -> Result<String, &'static str> {
    revision(&settings_values(s)?)
}
pub(super) fn settings(s: &Settings) -> Result<Value, &'static str> {
    let values = settings_values(s)?;
    let revision = revision(&values)?;
    Ok(
        json!({"settings":values,"withheld":WITHHELD_KEYS,"revision":revision,"platform":std::env::consts::OS}),
    )
}
// A static station-owned sidecar only. Distinguish never-saved from unreadable
// or malformed: a broken file must not look like an empty channel list.
fn programming(path: &Path, grid: &str) -> Result<Value, &'static str> {
    // Refuse special files before opening (a FIFO can block in open itself).
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() => return Err("applicationUnavailable"),
        Ok(meta) if meta.len() > 2 * 1024 * 1024 => return Err("applicationTooLarge"),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("applicationUnavailable"),
    }
    let mut bytes = Vec::new();
    match std::fs::File::open(path) {
        Ok(file) => {
            if !file
                .metadata()
                .map_err(|_| "applicationUnavailable")?
                .is_file()
            {
                return Err("applicationUnavailable");
            }
            file.take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "applicationUnavailable")?;
            if bytes.len() > 2 * 1024 * 1024 {
                return Err("applicationTooLarge");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(
                json!({"mygrid":grid,"projects":[],"revision":revision(&json!([]))?,"saved":false}),
            );
        }
        Err(_) => return Err("applicationUnavailable"),
    }
    let file: crate::RadioProgFile =
        serde_json::from_slice(&bytes).map_err(|_| "applicationUnavailable")?;
    if file.version != 1 {
        return Err("applicationUnavailable");
    }
    if file.projects.len() > 128
        || file
            .projects
            .iter()
            .map(|p| p.channels.len())
            .sum::<usize>()
            > 10_000
    {
        return Err("applicationTooLarge");
    }
    let projects = navigation::value(&file.projects)?;
    Ok(json!({"mygrid":grid,"revision":revision(&projects)?,"projects":projects,"saved":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn emit(name: &str, value: &Value) {
        if let Ok(path) = std::env::var("NEXUS_REMOTE_CONFIGURATION_FIXTURES") {
            let path = std::path::Path::new(&path);
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(
                path.join(format!("configuration-{name}.json")),
                serde_json::to_vec_pretty(value).unwrap(),
            )
            .unwrap();
        }
    }
    #[test]
    fn explicit_settings_schema_preserves_choices_and_omits_private_fields_before_serialization() {
        let mut s = Settings::default();
        s.mycall = "W1AW".into();
        s.mygrid = "FN31RX09".into();
        s.cw_wpm = 27;
        s.amp_follow_band = true;
        s.fd_class = "3A".into();
        s.ensure_radio_profiles();
        s.clublog_api_key = "must-not-leave-station".into();
        s.cloudlog_key = "must-not-leave-station".into();
        s.lotw_username = "private-account-label".into();
        let expected: Value = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        let result = settings(&s).unwrap();
        let view = result["settings"].as_object().unwrap();
        let actual_keys: std::collections::BTreeSet<&str> = expected
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let classified: std::collections::BTreeSet<&str> = SETTINGS_KEYS
            .iter()
            .chain(WITHHELD_KEYS)
            .copied()
            .filter(|k| actual_keys.contains(k))
            .collect();
        assert_eq!(
            actual_keys, classified,
            "a new native setting needs an explicit privacy classification"
        );
        assert_eq!(view.len(), SETTINGS_KEYS.len());
        for key in SETTINGS_KEYS {
            assert_eq!(view[*key], expected[*key], "{key}");
        }
        for key in WITHHELD_KEYS {
            assert!(!view.contains_key(*key), "{key}");
        }
        assert!(!result.to_string().contains("must-not-leave-station"));
        assert!(!result.to_string().contains("private-account-label"));
        assert_eq!(view["cwWpm"], 27);
        assert_eq!(view["ampFollowBand"], true);
        assert_eq!(
            serde_json::from_slice::<Value>(&serde_json::to_vec(&s).unwrap()).unwrap(),
            expected,
            "read must not change choices or legacy migration state"
        );
        emit("settings", &result);
        // An oversized omitted value must not consume the document budget at all.
        s.clublog_api_key = "x".repeat(3 * 1024 * 1024);
        assert!(settings(&s).is_ok());
        let before = result["revision"].clone();
        s.cw_wpm = 31;
        assert_ne!(settings(&s).unwrap()["revision"], before);
    }
    #[test]
    fn complete_saved_programming_list_roundtrips_without_writing_and_distinguishes_failure_from_empty(
    ) {
        let dir = std::env::temp_dir().join(format!(
            "remote-config-{}",
            super::super::snapshot_id().unwrap()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("radioprog.json");
        let absent = programming(&path, "FN31").unwrap();
        assert_eq!(absent["saved"], false);
        let channel:propagation::memchan::Channel=serde_json::from_value(json!({"id":"manual:146.9400:W1AW","name":"W1AW","rxMhz":146.94,"duplex":"minus","offsetMhz":0.6,"toneMode":"tone","rtoneHz":100.0,"ctoneHz":100.0,"dtcsCode":23,"mode":"fm","comment":"Station channel","dmrColorCode":null,"dmrTimeslot":null,"dmrTalkgroup":null,"dstarRpt1":null,"dstarRpt2":null,"source":null})).unwrap();
        let channels: Vec<_> = (0..1200)
            .map(|i| {
                let mut c = channel.clone();
                c.id = format!("manual:{i}");
                c.name = format!("CH{i:04}");
                c
            })
            .collect();
        let file = crate::RadioProgFile {
            version: 1,
            projects: vec![crate::RadioProgProject {
                id: "working".into(),
                name: "My channels".into(),
                created_utc: 1,
                updated_utc: 2,
                channels,
                ..Default::default()
            }],
        };
        let bytes = serde_json::to_vec(&file).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let result = programming(&path, "FN31").unwrap();
        assert_eq!(result["saved"], true);
        assert_eq!(
            result["projects"][0]["channels"].as_array().unwrap().len(),
            1200
        );
        assert_eq!(
            result["projects"],
            serde_json::to_value(&file.projects).unwrap()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        emit("programming", &result);
        std::fs::write(&path, b"broken file").unwrap();
        assert_eq!(programming(&path, "FN31"), Err("applicationUnavailable"));
        std::fs::write(&path, vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
        assert_eq!(programming(&path, "FN31"), Err("applicationTooLarge"));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(programming(&path, "FN31"), Err("applicationUnavailable"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
