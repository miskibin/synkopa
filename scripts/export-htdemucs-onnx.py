"""Export Demucs v4 (HTDemucs, MIT) to ONNX with the STFT and iSTFT left outside the graph.

`torch.stft`/`torch.istft` use complex tensors, which ONNX cannot carry, and they only run at
the start and the end of `HTDemucs.forward`. So the exported graph takes the mix twice -- as
audio and as its complex-as-channels spectrogram -- and returns, per source, the spectrogram
(still to be inverted) and the time-branch audio. `crates/stems` does the STFT, the iSTFT and
the sum in Rust (`stft`/`istft` there mirror `HTDemucs._spec`/`_ispec` below).

The graph has a fixed segment of `model.segment` seconds (7.8 s, 343980 frames at 44.1 kHz):
the transformer was trained on that length, and Demucs itself pads every chunk to it.

    python -m venv venv && venv/bin/pip install torch demucs onnx
    venv/bin/python scripts/export-htdemucs-onnx.py htdemucs out/htdemucs.onnx
Weights come from the Demucs release (MIT, https://github.com/facebookresearch/demucs).
Never commit the resulting .onnx; the app downloads it (see crates/stems).
"""

import argparse
import math
import sys

import onnx
import torch
from demucs.pretrained import get_model
from demucs.htdemucs import HTDemucs


class Core(torch.nn.Module):
    """`HTDemucs.forward` between `_spec` and `_ispec` (Demucs 4.0.1, eval, use_train_segment)."""

    def __init__(self, m: HTDemucs):
        super().__init__()
        self.m = m

    def forward(self, mix, mag):
        m = self.m
        x = mag
        B, C, Fq, T = x.shape
        mean = x.mean(dim=(1, 2, 3), keepdim=True)
        std = x.std(dim=(1, 2, 3), keepdim=True)
        x = (x - mean) / (1e-5 + std)
        xt = mix
        meant = xt.mean(dim=(1, 2), keepdim=True)
        stdt = xt.std(dim=(1, 2), keepdim=True)
        xt = (xt - meant) / (1e-5 + stdt)

        saved = []
        saved_t = []
        lengths = []
        lengths_t = []
        for idx, encode in enumerate(m.encoder):
            lengths.append(x.shape[-1])
            inject = None
            if idx < len(m.tencoder):
                lengths_t.append(xt.shape[-1])
                tenc = m.tencoder[idx]
                xt = tenc(xt)
                if not tenc.empty:
                    saved_t.append(xt)
                else:
                    inject = xt
            x = encode(x, inject)
            if idx == 0 and m.freq_emb is not None:
                frs = torch.arange(x.shape[-2], device=x.device)
                emb = m.freq_emb(frs).t()[None, :, :, None].expand_as(x)
                x = x + m.freq_emb_scale * emb
            saved.append(x)

        if m.crosstransformer:
            if m.bottom_channels:
                b, c, f, t = x.shape
                x = x.reshape(b, c, f * t)
                x = m.channel_upsampler(x)
                x = x.reshape(b, m.bottom_channels, f, t)
                xt = m.channel_upsampler_t(xt)
            x, xt = m.crosstransformer(x, xt)
            if m.bottom_channels:
                x = x.reshape(b, m.bottom_channels, f * t)
                x = m.channel_downsampler(x)
                x = x.reshape(b, c, f, t)
                xt = m.channel_downsampler_t(xt)

        for idx, decode in enumerate(m.decoder):
            skip = saved.pop(-1)
            x, pre = decode(x, skip, lengths.pop(-1))
            offset = m.depth - len(m.tdecoder)
            if idx >= offset:
                tdec = m.tdecoder[idx - offset]
                length_t = lengths_t.pop(-1)
                if tdec.empty:
                    assert pre.shape[2] == 1, pre.shape
                    pre = pre[:, :, 0]
                    xt, _ = tdec(pre, None, length_t)
                else:
                    skip = saved_t.pop(-1)
                    xt, _ = tdec(xt, skip, length_t)

        S = len(m.sources)
        x = x.view(B, S, -1, Fq, T)
        x = x * std[:, None] + mean[:, None]
        xt = xt.view(B, S, -1, mix.shape[-1])
        xt = xt * stdt[:, None] + meant[:, None]
        return x, xt


def spec(m, mix):
    """`HTDemucs._spec` + `_magnitude` (cac): [B, C, L] -> [B, 2C, nfft/2, ceil(L/hop)]."""
    z = m._spec(mix)
    return m._magnitude(z)


def full(m, core, mix):
    """The original forward rebuilt from the split pieces (the reference the export must match)."""
    mag = spec(m, mix)
    x, xt = core(mix, mag)
    zout = m._mask(None, x)
    return m._ispec(zout, mix.shape[-1]) + xt


def fp16_weights(mp):
    """Every float initializer of 16+ values stored as fp16 and cast back to fp32 by a Cast node in
    front of the graph: the file halves (174 MB -> 90 MB), ONNX Runtime folds the casts at load, and
    the network computes in fp32 as before. On the MUSDB18 test excerpts the stems differ from the
    fp32 export by < 1e-4 and the SDR by < 0.01 dB."""
    import numpy as np
    from onnx import TensorProto, helper, numpy_helper

    g = mp.graph
    inits, casts = [], []
    for init in g.initializer:
        if init.data_type == TensorProto.FLOAT and int(np.prod(init.dims)) >= 16:
            h = numpy_helper.from_array(numpy_helper.to_array(init).astype(np.float16), init.name + "__fp16")
            inits.append(h)
            casts.append(helper.make_node("Cast", [h.name], [init.name], to=TensorProto.FLOAT, name=init.name + "__cast"))
        else:
            inits.append(init)
    nodes = list(g.node)
    del g.initializer[:]
    g.initializer.extend(inits)
    del g.node[:]
    g.node.extend(casts + nodes)
    onnx.checker.check_model(mp)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("name", help="htdemucs, htdemucs_6s or htdemucs_ft (one ONNX per bag member)")
    ap.add_argument("out")
    ap.add_argument("--opset", type=int, default=17)
    ap.add_argument("--fp16-weights", action="store_true",
                    help="store the weights as fp16 (half the download); the graph still computes in fp32")
    a = ap.parse_args()

    bag = get_model(a.name)
    for i, m in enumerate(bag.models):
        assert isinstance(m, HTDemucs) and m.cac and m.use_train_segment, type(m)
        m.eval()
        L = int(m.segment * m.samplerate)
        mix = torch.randn(1, m.audio_channels, L) * 0.1
        core = Core(m).eval()
        with torch.no_grad():
            ref = m(mix)
            got = full(m, core, mix)
        err = (ref - got).abs().max().item()
        print(f"[{i}] sources={m.sources} segment={L} split-vs-original max|d|={err:.2e}", file=sys.stderr)
        assert err < 1e-4
        out = a.out if len(bag.models) == 1 else a.out.replace(".onnx", f".{i}.onnx")
        mag = spec(m, mix)
        torch.onnx.export(
            core,
            (mix, mag),
            out,
            input_names=["mix", "spec"],
            output_names=["spec_out", "wave_out"],
            opset_version=a.opset,
            dynamo=False,
        )
        # The Rust side reads the stem names and order from the model, so a 6-source export works too.
        mp = onnx.load(out, load_external_data=True)
        meta = {"sources": ",".join(m.sources), "samplerate": str(m.samplerate), "segment": str(L),
                "nfft": str(m.nfft), "hop": str(m.hop_length)}
        for k, v in meta.items():
            e = mp.metadata_props.add()
            e.key, e.value = k, v
        if a.fp16_weights:
            fp16_weights(mp)
        onnx.save(mp, out)
        print(f"wrote {out}: spec {tuple(mag.shape)}, weights {bag.weights[i] if bag.weights else None}", file=sys.stderr)


if __name__ == "__main__":
    main()
