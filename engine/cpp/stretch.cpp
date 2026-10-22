// Envoltorio de C sobre Signalsmith Stretch para el motor del celular
// (src/stretch.rs). Audio intercalado en estereo, como lo mezcla el motor.
#include "signalsmith-stretch/signalsmith-stretch.h"

#include <cstddef>

namespace {

// Vista por canal de un bufer intercalado: Signalsmith lee `buffers[canal][i]`.
struct Interleaved {
    float *data;
    int channels;

    struct Channel {
        float *data;
        int channel;
        int stride;
        float &operator[](int i) const { return data[i * stride + channel]; }
    };

    Channel operator[](int channel) const { return Channel{data, channel, channels}; }
};

struct Stretcher {
    signalsmith::stretch::SignalsmithStretch<float> stretch;
    int channels;
};

}  // namespace

extern "C" {

void *dp_stretch_new(int channels, float rate) {
    auto *s = new Stretcher;
    s->channels = channels;
    // el calculo repartido entre llamadas: sin picos en un trozo (en un
    // telefono, el peor trozo es el que corta)
    s->stretch.presetDefault(channels, rate, true);
    return s;
}

void dp_stretch_free(void *s) { delete static_cast<Stretcher *>(s); }

void dp_stretch_reset(void *s) { static_cast<Stretcher *>(s)->stretch.reset(); }

int dp_stretch_input_latency(void *s) {
    return static_cast<Stretcher *>(s)->stretch.inputLatency();
}

int dp_stretch_output_latency(void *s) {
    return static_cast<Stretcher *>(s)->stretch.outputLatency();
}

void dp_stretch_set_pitch(void *s, float semitones) {
    static_cast<Stretcher *>(s)->stretch.setTransposeSemitones(semitones);
}

void dp_stretch_seek(void *s, float *input, int frames, double rate) {
    auto *e = static_cast<Stretcher *>(s);
    e->stretch.seek(Interleaved{input, e->channels}, frames, rate);
}

void dp_stretch_process(void *s, float *input, int in_frames, float *output, int out_frames) {
    auto *e = static_cast<Stretcher *>(s);
    e->stretch.process(Interleaved{input, e->channels}, in_frames,
                       Interleaved{output, e->channels}, out_frames);
}

}  // extern "C"
