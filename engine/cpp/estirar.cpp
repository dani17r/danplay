// Envoltorio de C sobre Signalsmith Stretch para el motor del celular
// (src/stretch.rs). Audio intercalado en estereo, como lo mezcla el motor.
#include "signalsmith-stretch/signalsmith-stretch.h"

#include <cstddef>

namespace {

// Vista por canal de un bufer intercalado: Signalsmith lee `buffers[canal][i]`.
struct Intercalado {
    float *data;
    int channels;

    struct Canal {
        float *data;
        int channel;
        int stride;
        float &operator[](int i) const { return data[i * stride + channel]; }
    };

    Canal operator[](int channel) const { return Canal{data, channel, channels}; }
};

struct Estirar {
    signalsmith::stretch::SignalsmithStretch<float> stretch;
    int channels;
};

}  // namespace

extern "C" {

void *dp_estirar_nuevo(int channels, float rate) {
    auto *s = new Estirar;
    s->channels = channels;
    // el calculo repartido entre llamadas: sin picos en un trozo (en un
    // telefono, el peor trozo es el que corta)
    s->stretch.presetDefault(channels, rate, true);
    return s;
}

void dp_estirar_borrar(void *s) { delete static_cast<Estirar *>(s); }

void dp_estirar_reiniciar(void *s) { static_cast<Estirar *>(s)->stretch.reset(); }

int dp_estirar_latencia_entrada(void *s) {
    return static_cast<Estirar *>(s)->stretch.inputLatency();
}

int dp_estirar_latencia_salida(void *s) {
    return static_cast<Estirar *>(s)->stretch.outputLatency();
}

void dp_estirar_tono(void *s, float semitones) {
    static_cast<Estirar *>(s)->stretch.setTransposeSemitones(semitones);
}

void dp_estirar_preparar(void *s, float *input, int frames, double rate) {
    auto *e = static_cast<Estirar *>(s);
    e->stretch.seek(Intercalado{input, e->channels}, frames, rate);
}

void dp_estirar_procesar(void *s, float *input, int in_frames, float *output, int out_frames) {
    auto *e = static_cast<Estirar *>(s);
    e->stretch.process(Intercalado{input, e->channels}, in_frames,
                       Intercalado{output, e->channels}, out_frames);
}

}  // extern "C"
