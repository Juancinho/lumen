# Chapter 4 - Forecasting monthly demand

This chapter compares seasonal ARIMA, exponential smoothing and a gradient-boosted model
for forecasting monthly sales. Seasonality is strong (period 12); the series is made
stationary by seasonal differencing. Models are evaluated with rolling-origin
cross-validation, reporting MAPE over a twelve-month horizon. Exponential smoothing wins
on short horizons; the boosted model is best when promotions are known in advance.
