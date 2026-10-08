# Gradient descent - lecture notes

Gradient descent updates parameters against the gradient: w <- w - eta * grad L(w).

Convergence: for a convex loss with L-smooth gradients, a fixed step eta <= 1/L
guarantees the loss decreases every step and converges at rate O(1/t). Too large a
learning rate makes the iterates oscillate or diverge; too small makes progress slow.
Momentum and adaptive methods (Adam) change the effective step per parameter.
