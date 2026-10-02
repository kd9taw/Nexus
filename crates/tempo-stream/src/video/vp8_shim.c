/* Nexus: a thin VP8 encoder over libvpx, for Remote as a stream.
 *
 * Nexus-owned. It exists so that no libvpx struct layout is ever copied into Rust by hand: this
 * file is compiled against the headers of the libvpx it links (build.rs; the pinned source build
 * in scripts/build-windows-cross.sh), and Rust sees only the five functions below.
 *
 * The encoder is configured for a live picture of an application window:
 *   - realtime, one pass, no lookahead (g_lag_in_frames 0), one thread, constant bit rate;
 *   - screen content mode, and a static threshold, so a still window costs almost nothing;
 *   - never drops a frame on its own (rc_dropframe_thresh 0): the station decides what is sent;
 *   - keyframes when asked (the first frame, a size change, a lost frame, the page's PLI/FIR),
 *     and otherwise at most every 3000 frames;
 *   - timestamps in milliseconds from the encoder's opening.
 */
#include <stddef.h>
#include <stdlib.h>

#include "vpx/vp8cx.h"
#include "vpx/vpx_encoder.h"

typedef struct nexus_vp8 {
  vpx_codec_ctx_t codec;
  vpx_codec_iter_t iter;
  unsigned width, height;
} nexus_vp8;

/* The linked libvpx's own version string, e.g. "v1.17.0". */
const char *nexus_vp8_version(void) { return vpx_codec_version_str(); }

/* A VP8 encoder for width x height pictures at kbps kilobits a second, or NULL. */
nexus_vp8 *nexus_vp8_open(unsigned width, unsigned height, unsigned kbps) {
  vpx_codec_enc_cfg_t cfg;
  nexus_vp8 *enc;

  if (width == 0 || height == 0 || kbps == 0) return NULL;
  if (vpx_codec_enc_config_default(vpx_codec_vp8_cx(), &cfg, 0) != VPX_CODEC_OK) return NULL;
  cfg.g_w = width;
  cfg.g_h = height;
  cfg.g_timebase.num = 1;
  cfg.g_timebase.den = 1000;
  cfg.g_threads = 1;
  cfg.g_lag_in_frames = 0;
  cfg.g_pass = VPX_RC_ONE_PASS;
  cfg.g_error_resilient = VPX_ERROR_RESILIENT_DEFAULT;
  cfg.rc_end_usage = VPX_CBR;
  cfg.rc_target_bitrate = kbps;
  cfg.rc_dropframe_thresh = 0;
  cfg.rc_min_quantizer = 2;
  cfg.rc_max_quantizer = 56;
  cfg.rc_undershoot_pct = 100;
  cfg.rc_overshoot_pct = 15;
  cfg.rc_buf_initial_sz = 500;
  cfg.rc_buf_optimal_sz = 600;
  cfg.rc_buf_sz = 1000;
  cfg.kf_mode = VPX_KF_AUTO;
  cfg.kf_min_dist = 0;
  cfg.kf_max_dist = 3000;

  enc = (nexus_vp8 *)calloc(1, sizeof(*enc));
  if (enc == NULL) return NULL;
  enc->width = width;
  enc->height = height;
  if (vpx_codec_enc_init(&enc->codec, vpx_codec_vp8_cx(), &cfg, 0) != VPX_CODEC_OK) {
    free(enc);
    return NULL;
  }
  if (vpx_codec_control(&enc->codec, VP8E_SET_CPUUSED, -8) != VPX_CODEC_OK ||
      vpx_codec_control(&enc->codec, VP8E_SET_SCREEN_CONTENT_MODE, 1) != VPX_CODEC_OK ||
      vpx_codec_control(&enc->codec, VP8E_SET_STATIC_THRESHOLD, 1) != VPX_CODEC_OK ||
      vpx_codec_control(&enc->codec, VP8E_SET_NOISE_SENSITIVITY, 0) != VPX_CODEC_OK ||
      vpx_codec_control(&enc->codec, VP8E_SET_TOKEN_PARTITIONS, VP8_ONE_TOKENPARTITION) !=
          VPX_CODEC_OK ||
      vpx_codec_control(&enc->codec, VP8E_SET_MAX_INTRA_BITRATE_PCT, 300) != VPX_CODEC_OK) {
    vpx_codec_destroy(&enc->codec);
    free(enc);
    return NULL;
  }
  return enc;
}

/* Encode one I420 picture shown at pts_ms. 0 on success; the packets are then read with
 * nexus_vp8_next until it returns 0, and stay valid until the next encode. */
int nexus_vp8_encode(nexus_vp8 *enc, const unsigned char *y, int y_stride, const unsigned char *u,
                     int u_stride, const unsigned char *v, int v_stride, long long pts_ms,
                     unsigned long duration_ms, int keyframe) {
  vpx_image_t image;

  if (enc == NULL || y == NULL || u == NULL || v == NULL) return -1;
  if (vpx_img_wrap(&image, VPX_IMG_FMT_I420, enc->width, enc->height, 1, (unsigned char *)y) ==
      NULL)
    return -1;
  image.planes[VPX_PLANE_Y] = (unsigned char *)y;
  image.planes[VPX_PLANE_U] = (unsigned char *)u;
  image.planes[VPX_PLANE_V] = (unsigned char *)v;
  image.stride[VPX_PLANE_Y] = y_stride;
  image.stride[VPX_PLANE_U] = u_stride;
  image.stride[VPX_PLANE_V] = v_stride;
  enc->iter = NULL;
  if (vpx_codec_encode(&enc->codec, &image, (vpx_codec_pts_t)pts_ms,
                       duration_ms > 0 ? duration_ms : 1, keyframe ? VPX_EFLAG_FORCE_KF : 0,
                       VPX_DL_REALTIME) != VPX_CODEC_OK)
    return -1;
  return 0;
}

/* The next packet of the last encode: 1 with *data, *size and *keyframe set, or 0 when none is
 * left. */
int nexus_vp8_next(nexus_vp8 *enc, const unsigned char **data, size_t *size, int *keyframe) {
  const vpx_codec_cx_pkt_t *packet;

  if (enc == NULL) return 0;
  while ((packet = vpx_codec_get_cx_data(&enc->codec, &enc->iter)) != NULL) {
    if (packet->kind != VPX_CODEC_CX_FRAME_PKT) continue;
    *data = (const unsigned char *)packet->data.frame.buf;
    *size = packet->data.frame.sz;
    *keyframe = (packet->data.frame.flags & VPX_FRAME_IS_KEY) != 0;
    return 1;
  }
  return 0;
}

void nexus_vp8_close(nexus_vp8 *enc) {
  if (enc == NULL) return;
  vpx_codec_destroy(&enc->codec);
  free(enc);
}
