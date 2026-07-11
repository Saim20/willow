#!/usr/bin/env python3
"""Encode plain-text KWS phrases into sherpa-onnx keywords.txt format."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tokens", required=True, help="Path to tokens.txt")
    parser.add_argument("--bpe-model", required=True, help="Path to bpe.model")
    parser.add_argument("--input", required=True, help="Plain-text phrases, one per line")
    parser.add_argument("--output", required=True, help="Encoded keywords output path")
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    try:
        import sentencepiece as spm
    except ImportError:
        print("sentencepiece is required to encode keywords", file=sys.stderr)
        return 1

    tokens_path = Path(args.tokens)
    bpe_model_path = Path(args.bpe_model)
    input_path = Path(args.input)
    output_path = Path(args.output)

    if not tokens_path.is_file() or not bpe_model_path.is_file() or not input_path.is_file():
        print("tokens, bpe-model, and input files must exist", file=sys.stderr)
        return 1

    token_table = {}
    with tokens_path.open(encoding="utf-8") as handle:
        for line in handle:
            parts = line.strip().split()
            if len(parts) == 2:
                token_table[parts[0]] = int(parts[1])

    sp = spm.SentencePieceProcessor()
    sp.Load(str(bpe_model_path))

    encoded_lines = []
    for raw_line in input_path.read_text(encoding="utf-8").splitlines():
        phrase = raw_line.strip()
        if not phrase:
            continue
        pieces = sp.encode(phrase.upper(), out_type=str)
        if any(piece not in token_table for piece in pieces):
            missing = [piece for piece in pieces if piece not in token_table]
            print(f"skipping {phrase!r}: missing tokens {missing}", file=sys.stderr)
            continue
        tag = phrase.upper().replace(" ", "_")
        encoded_lines.append(f"{' '.join(pieces)} @{tag}")

    if not encoded_lines:
        print("no phrases could be encoded", file=sys.stderr)
        return 1

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text("\n".join(encoded_lines) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
