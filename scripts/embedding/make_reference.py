#!/usr/bin/env python3
"""Generate reference embeddings for the T006 fidelity check.

Dev tool only (Python is never a production dependency, ADR-005). Uses the
onnx-community fp32 export through ONNX Runtime + HF `tokenizers`, independent of
the Rust code path, so Rust backends can be compared against it.

    python scripts/embedding/make_reference.py --model-dir <dir> --variant model \
        --out fixtures/embedding/reference-eg2-onnx-fp32-d256.json

<dir> is a local copy of https://huggingface.co/onnx-community/embeddinggemma-2-ONNX
(tokenizer.json + onnx/<variant>.onnx[_data]).
"""

import argparse
import hashlib
import json
import pathlib
import sys

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

QUERY_PREFIX = "task: search result | query: "  # PromptFormat::EMBEDDINGGEMMA_RETRIEVAL_V1
DIM = 256  # EmbeddingProfile::DEFAULT


def doc_prompt(title, text):
    title = (title or "").strip() or "none"
    return f"title: {title} | text: {text}"


def sha256(path, limit=None):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model-dir", required=True, type=pathlib.Path)
    ap.add_argument("--variant", default="model", help="model | model_quantized | model_q4 | ...")
    ap.add_argument("--corpus", default="fixtures/embedding/corpus.json", type=pathlib.Path)
    ap.add_argument("--out", required=True, type=pathlib.Path)
    args = ap.parse_args()

    corpus = json.loads(args.corpus.read_text(encoding="utf-8"))
    tok = Tokenizer.from_file(str(args.model_dir / "tokenizer.json"))
    onnx_path = args.model_dir / "onnx" / f"{args.variant}.onnx"
    sess = ort.InferenceSession(str(onnx_path), providers=["CPUExecutionProvider"])
    empty = np.zeros((0, 512), np.float32)

    def embed(text):
        ids = np.array([tok.encode(text).ids], np.int64)
        out = sess.run(
            ["sentence_embedding"],
            {
                "input_ids": ids,
                "attention_mask": np.ones_like(ids),
                "image_features": empty,
                "video_features": empty,
                "audio_features": empty,
            },
        )[0][0]
        head = out[:DIM].astype(np.float64)
        return (head / np.linalg.norm(head)).astype(np.float32)

    queries = [(q["id"], QUERY_PREFIX + q["text"]) for q in corpus["queries"]]
    docs = [(d["id"], doc_prompt(d.get("title"), d["text"])) for d in corpus["documents"]]
    qv = {i: embed(t) for i, t in queries}
    dv = {i: embed(t) for i, t in docs}

    doc_ids = [i for i, _ in docs]
    dm = np.stack([dv[i] for i in doc_ids])
    ranks = {}
    for q in corpus["queries"]:
        scores = dm @ qv[q["id"]]
        order = [doc_ids[j] for j in np.argsort(-scores)]
        ranks[q["id"]] = order.index(q["relevant"]) + 1
    recall1 = sum(r == 1 for r in ranks.values()) / len(ranks)
    mrr = sum(1 / r for r in ranks.values()) / len(ranks)

    report = {
        "schema_version": 1,
        "model": "onnx-community/embeddinggemma-2-ONNX",
        "variant": args.variant,
        "onnx_sha256": sha256(onnx_path),
        "onnx_data_sha256": sha256(str(onnx_path) + "_data"),
        "tokenizer_sha256": sha256(args.model_dir / "tokenizer.json"),
        "onnxruntime": ort.__version__,
        "corpus_version": corpus["version"],
        "dim": DIM,
        "prompts": "embeddinggemma-retrieval@1",
        "retrieval": {"recall_at_1": recall1, "mrr": mrr, "rank_of_relevant": ranks},
        "vectors": {
            **{i: [round(float(x), 7) for x in v] for i, v in qv.items()},
            **{i: [round(float(x), 7) for x in v] for i, v in dv.items()},
        },
    }
    args.out.write_text(json.dumps(report, indent=1) + "\n", encoding="utf-8")
    print(f"{args.variant}: recall@1={recall1:.3f} mrr={mrr:.3f} -> {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
