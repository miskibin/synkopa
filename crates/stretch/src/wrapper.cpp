// C ABI over Signalsmith Stretch for offline, whole-buffer time-stretching.
#include <signalsmith-stretch.h>

#include <cstddef>
#include <new>

extern "C" {

// Stretches `channels` planar input buffers of `in_len` frames into planar output
// buffers of `out_len` frames, keeping pitch, and transposes them by `semitones`
// (0 = none). Returns 0 on success, 1 when the output is too short for the algorithm
// (the caller pads), 2 on allocation failure.
int wave_stretch_exact(float sample_rate, int channels, const float *const *input, size_t in_len, float *const *output, size_t out_len, float semitones) {
    try {
        signalsmith::stretch::SignalsmithStretch<float> s;
        s.presetDefault(channels, sample_rate);
        if (semitones != 0.0f) s.setTransposeSemitones(semitones);
        // `exact` takes channel-indexable buffers; it reads input only
        float **in = const_cast<float **>(input);
        float *const *out = output;
        return s.exact(in, (int)in_len, out, (int)out_len) ? 0 : 1;
    } catch (const std::bad_alloc &) {
        return 2;
    } catch (...) {
        return 2;
    }
}

// Output latency of the default preset, so the caller knows the minimum output length.
size_t wave_stretch_min_output(float sample_rate, int channels) {
    signalsmith::stretch::SignalsmithStretch<float> s;
    s.presetDefault(channels, sample_rate);
    return (size_t)s.outputLatency() * 2;
}

namespace {

// A read-only view over one planar input channel that returns 0 past `length`, so the
// stretcher can read its full latency window at the end of the input.
struct ZeroPadded {
    const float *const *input;
    int offset, length;
    struct Channel {
        const ZeroPadded &z;
        int c;
        float operator[](int i) const {
            int j = z.offset + i;
            return j < z.length ? z.input[c][j] : 0.0f;
        }
    };
    Channel operator[](int c) const { return {*this, c}; }
};

// Produces exactly `n` output frames at `out + done` from `n_in` input frames starting at
// `in`'s offset + `done_in` (the stretcher's speed is set by the call's input/output pacing).
inline void paced_process(signalsmith::stretch::SignalsmithStretch<float> &s, const ZeroPadded &in, int done_in, int n_in, float *const *out, int done_out, int n) {
    struct InView {
        const ZeroPadded &z;
        int off;
        struct Channel {
            const InView &v;
            int c;
            float operator[](int i) const { return v.z[c][v.off + i]; }
        };
        Channel operator[](int c) const { return {*this, c}; }
    } in_view{in, done_in};
    struct OutView {
        float *const *out;
        int off;
        struct Channel {
            const OutView &v;
            int c;
            float &operator[](int i) const { return v.out[c][v.off + i]; }
        };
        Channel operator[](int c) const { return {*this, c}; }
    } out_view{out, done_out};
    s.process(in_view, n_in, out_view, n);
}

}

// Piecewise time-stretch: `anchors` are (input frame, output frame) pairs, both strictly
// increasing integers starting (0, 0), the last at (in_total-anchored, out_len-anchored);
// between anchors the stretch is the segment's slope, so a marker's audio lands where the
// map says. Renders exactly `out_len` frames in one continuous stretcher (no seams at
// anchors — the algorithm's windows cross them), then trims its head latency exactly like
// `exact` does. Returns 0 on success, 1 when the output is too short for the algorithm
// (the caller pads), 2 on failure, 3 on bad anchors.
int wave_stretch_warp(float sample_rate, int channels, const float *const *input, size_t in_len, float *const *output, size_t out_len, const double *anchors, size_t n_anchors, float semitones) {
    if (n_anchors < 2 || anchors[0] != 0.0 || anchors[1] != 0.0) return 3;
    for (size_t i = 2; i < n_anchors * 2; i += 2) {
        if (!(anchors[i] > anchors[i - 2]) || !(anchors[i + 1] > anchors[i - 1])) return 3;
    }
    if (llround(anchors[n_anchors * 2 - 1]) != (long long)out_len) return 3;
    try {
        signalsmith::stretch::SignalsmithStretch<float> s;
        s.presetDefault(channels, sample_rate);
        if (semitones != 0.0f) s.setTransposeSemitones(semitones);
        int in_total = (int)in_len, out_total = (int)out_len;
        if (out_total < s.outputLatency() * 2) return 1;
        // `exact`'s positioning: the first `inputLatency` frames are pre-roll context, the
        // mapped range reads from there and is zero-padded past the input's end. Both views
        // are zero-padded, so an input shorter than the latencies reads zeros, not past its
        // buffers (seek takes the head view: its pre-roll is the input's own first frames).
        ZeroPadded head{input, 0, in_total};
        s.seek(head, s.inputLatency(), (double)in_total / (double)out_total);
        ZeroPadded in{input, s.inputLatency(), in_total};

        int done_in = 0, done_out = 0;
        for (size_t a = 1; a < n_anchors; ++a) {
            int seg_in_end = (int)anchors[a * 2], seg_out_end = (int)anchors[a * 2 + 1];
            int seg_in = seg_in_end - done_in, seg_out = seg_out_end - done_out;
            if (seg_in <= 0 || seg_out <= 0) return 3;
            if (seg_in <= 4096) {
                paced_process(s, in, done_in, seg_in, output, done_out, seg_out);
                done_in = seg_in_end;
                done_out = seg_out_end;
            } else {
                // walk in fixed chunks; the final chunk absorbs the rounding so the
                // cumulative output lands on the anchor exactly
                while (done_in < seg_in_end) {
                    int chunk = std::min(4096, seg_in_end - done_in);
                    int n;
                    if (done_in + chunk >= seg_in_end) {
                        n = seg_out_end - done_out;
                    } else {
                        double through = double(done_in + chunk - (seg_in_end - seg_in)) / double(seg_in);
                        n = (int)llround(through * seg_out) - (done_out - (seg_out_end - seg_out));
                    }
                    if (n <= 0) return 3;
                    paced_process(s, in, done_in, chunk, output, done_out, n);
                    done_in += chunk;
                    done_out += n;
                }
            }
            if (done_out != seg_out_end) return 3;
        }
        // the head latency fold-back, exactly as `exact` does it
        int lat = s.outputLatency();
        for (int c = 0; c < channels; ++c) {
            auto &&channel = output[c];
            for (int i = 0; i < std::min<int>(out_total - lat, lat); ++i) {
                channel[i + lat] -= channel[lat - 1 - i];
            }
            for (int i = 0; i < out_total - lat; ++i) {
                channel[i] = channel[i + lat];
            }
        }
        // and the final latency-sized chunk goes at the very end
        struct TailView {
            float *const *out;
            int off;
            struct Channel {
                const TailView &v;
                int c;
                float &operator[](int i) const { return v.out[c][v.off + i]; }
            };
            Channel operator[](int c) const { return {*this, c}; }
        } tail{output, out_total - lat};
        s.flush(tail, lat);
        return 0;
    } catch (const std::bad_alloc &) {
        return 2;
    } catch (...) {
        return 2;
    }
}
}
