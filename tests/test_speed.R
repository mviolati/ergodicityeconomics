coin_toss()

players <- 10^3
rounds <- 10

df <- coin_toss(0.6, 1.5, players, rounds)

df <-  pivot_longer(
  mutate(
    rowid_to_column(df, "players"),
    ensemble_avg = rowMeans(df),
    ensemble_median = apply(df, 1, median),
    expected_value = EV ^ rounds
  ),
  cols = !1,
  names_to = "rounds"
)

df <- coin_transform(df)

coin_plot(df)

matplot(df,
        type = "l", lwd = 0.5, lty = 1, log = "y",
        xlab = 'Months', ylab = 'Millions',
        main = 'Projected Value of Initial Capital')
