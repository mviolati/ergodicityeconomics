library(ergodicityeconomics)
library(microbenchmark)
library(profvis)
library(tidyverse)
library(scales)

df <- coin_transform(coin_toss(0.6, 1.5, 10^4, 10^3))

df |>
  dplyr::mutate(ensemble_avg = rowMeans(df),
                ensemble_median = apply(df, 1, median)) |>
  tibble::rowid_to_column("rounds") |>
  dplyr::mutate(expected_value = EV ^ rounds) |>
  tidyr::pivot_longer(cols = !1, names_to = "individual")

# !! very slow !! with 10^4
# !! plotting is the biggest bottleneck now !!
coin_toss(0.6, 1.5, 10 ^ 3, 10 ^ 3) |>
  coin_transform() |>
  coin_plot()

profvis({
  focus <- c("ensemble_avg", "expected_value", "ensemble_median")

  ggplot() +
    geom_line(
      subset(df, !individual %in% focus),
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
})
