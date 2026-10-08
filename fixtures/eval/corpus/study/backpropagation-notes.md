# Backpropagation

The chain rule gives the gradient of the loss with respect to every weight: errors flow
backwards layer by layer, each layer multiplying by its local derivative. Vanishing
gradients appear with saturating activations; ReLU and residual connections help.
