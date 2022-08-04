library(ergodicityeconomics)
library(microbenchmark)
library(profvis)
library(tidyverse)

df <- coin_toss(0.6, 1.5, 10, 10)

df |>
  dplyr::mutate(ensemble_avg = rowMeans(df),
                ensemble_median = apply(df, 1, median)) |>
  tibble::rowid_to_column("rounds") |>
  dplyr::mutate(expected_value = EV ^ rounds) |>
  tidyr::pivot_longer(cols = !starts_with("r"), names_to = "individual")

# !! very slow !! with 10^4
coin_toss(0.6, 1.5, 10 ^ 4, 10 ^ 4) |>
  coin_transform() |>
  coin_plot()



profvis({
  coin_toss(0.6, 1.5, 10 ^ 4, 10 ^ 4) |>
    coin_transform() |>
    coin_plot()
})
