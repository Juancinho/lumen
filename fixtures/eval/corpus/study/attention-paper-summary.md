# Summary: the original Transformer paper

The paper replaces recurrence with self-attention: every token attends to every other
token, so sequences are processed in parallel. Multi-head attention lets the model look
at different relations at once; positional encodings add word order. The encoder-decoder
model set a new state of the art in machine translation while training much faster than
recurrent networks.
