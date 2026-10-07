#!/usr/bin/env python3
"""Embed a text corpus into raw f32 files for `lumen-bench ann --vectors` (T008).

Dev tool only. Documents use the Document prompt, queries the SearchQuery prompt (the
asymmetric setup Lumen uses), truncated to --dim and L2-normalized like `Embedder`.

    python scripts/embedding/embed_corpus.py --model-dir <onnx export> --texts docs.txt \
        --queries queries.txt --out-docs docs.f32 --out-queries queries.f32
"""

import argparse
import sys
import time

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

QUERY = "task: search result | query: "
DOC = "title: none | text: "


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model-dir", required=True)
    ap.add_argument("--variant", default="model")
    ap.add_argument("--texts", required=True)
    ap.add_argument("--queries", required=True)
    ap.add_argument("--out-docs", required=True)
    ap.add_argument("--out-queries", required=True)
    ap.add_argument("--dim", type=int, default=256)
    ap.add_argument("--batch", type=int, default=32)
    a = ap.parse_args()

    tok = Tokenizer.from_file(f"{a.model_dir}/tokenizer.json")
    sess = ort.InferenceSession(f"{a.model_dir}/onnx/{a.variant}.onnx", providers=["CPUExecutionProvider"])
    empty = np.zeros((0, 512), np.float32)

    def embed(texts, prefix, out_path):
        out = open(out_path, "wb")
        t0 = time.time()
        for i in range(0, len(texts), a.batch):
            encs = tok.encode_batch([prefix + t for t in texts[i : i + a.batch]])
            seq = max(len(e.ids) for e in encs)
            ids = np.zeros((len(encs), seq), np.int64)
            mask = np.zeros_like(ids)
            for r, e in enumerate(encs):
                ids[r, : len(e.ids)] = e.ids
                mask[r, : len(e.ids)] = 1
            emb = sess.run(["sentence_embedding"], {"input_ids": ids, "attention_mask": mask,
                           "image_features": empty, "video_features": empty, "audio_features": empty})[0]
            head = emb[:, : a.dim].astype(np.float64)
            head /= np.linalg.norm(head, axis=1, keepdims=True)
            out.write(head.astype("<f4").tobytes())
            if (i // a.batch) % 20 == 0:
                print(f"  {out_path}: {i + len(encs)}/{len(texts)} ({time.time() - t0:.0f}s)", file=sys.stderr)
        out.close()

    docs = [l.rstrip("\n") for l in open(a.texts, encoding="utf-8") if l.strip()]
    queries = [l.rstrip("\n") for l in open(a.queries, encoding="utf-8") if l.strip()]
    embed(queries, QUERY, a.out_queries)
    embed(docs, DOC, a.out_docs)
    print(f"docs={len(docs)} queries={len(queries)} dim={a.dim}", file=sys.stderr)


if __name__ == "__main__":
    main()
