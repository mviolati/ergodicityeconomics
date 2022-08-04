library(ergodicityeconomics)
library(microbenchmark)
library(profvis)
library(tidyverse)
library(scales)
library(microbenchmark)


# Microbench Package ----

df  <- coin_toss(0.6, 1.5, 10 ^ 3, 10 ^ 3)
df1 <- coin_transform(df)

microbenchmark(
coin_toss(0.6, 1.5, 10 ^ 3, 10 ^ 3),
coin_transform(df),
coin_plot(df1)
)

# Profiling Package's Functions ----

profvis({
  coin_toss(0.6, 1.5, 10 ^ 3, 10 ^ 3) |>
    coin_transform() |>
    coin_plot()

})

# Check ggplot bottlenecks ----

df  <- coint_transform(coin_toss(0.6, 1.5, 10 ^ 3, 10 ^ 3))

profvis({
  focus <- c("ensemble_avg", "expected_value", "ensemble_median")

  g <- ggplot() +
    geom_line(
      subset(df,!individual %in% focus),
      mapping = aes(x = rounds, y = value, group = individual),
      size = 0.5,
      color = "grey"
    ) +
    geom_line(
      subset(df, individual %in% focus),
      mapping = aes(x = rounds, y = value, color = individual),
      size = 1
    ) +
    scale_y_log10(
      breaks = trans_breaks("log10", function(x)
        10 ^ x),
      labels = trans_format("log10", math_format(10 ^ .x))
    ) +
    theme_bw()
  print(g)
})
