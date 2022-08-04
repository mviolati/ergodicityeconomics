
<!-- README.md is generated from README.Rmd. Please edit that file -->

# Ergodicity Economics

## Replicating Example Gamble

First objective was to replicate Fig. 2 of this
[paper](https://rdcu.be/cS2t3).

## Code Optimization

In order to try to optimise code for a larger tibble, I have momentarily
divided the code in 3 different phases: \* Tibble Generation \*
Transformation \* Plotting

The code below is taking the same input given to the paper.

### Tibble Generation

The bottleneck here is replicate which is nesting sapply that is nesting
lapply. So with a large number of players a lot of vectors are created
through lapply.

``` r
assign("EV", ((0.6 * 0.5) + (1.5 * 0.5)), envir = .GlobalEnv)
df <- as_tibble(replicate(150,
                          cumprod(sample(
                            c(0.6, 1.5),
                            size = 1000,
                            replace = TRUE
                          ))))
```

### Transformation

The bottleneck is on making the tibble longer in order to lett ggplot
properly work.

``` r
df <- df |>
  mutate(ensemble_avg = rowMeans(df),
         ensemble_median = apply(df, 1, median)) |>
  rowid_to_column("rounds") |>
  mutate(expected_value = EV ^ rounds) |>
  pivot_longer(cols = !1, names_to = "individual")
```

### Plotting

Plotting is the biggest bottleneck.

``` r
focus <- c("ensemble_avg", "expected_value", "ensemble_median")

ggplot() +
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
```

![](README-unnamed-chunk-4-1.png)<!-- -->