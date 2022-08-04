#' Description pending
#'
#' @param x
#'
#' @importFrom ggplot2 ggplot aes geom_line scale_y_log10 theme_bw
#' @importFrom scales trans_breaks trans_format math_format
#'
#' @export

coin_plot <- function(x) {
  focus <- c("ensemble_avg", "expected_value", "ensemble_median")

  ggplot() +
    geom_line(
      subset(x, !individual %in% focus),
      mapping = aes(x = rounds, y = value, group = individual),
      size = 0.5,
      color = "grey"
    ) +
    geom_line(
      subset(x, in
             dividual %in% focus),
      mapping = aes(x = rounds, y = value, color = individual),
      size = 1
    ) +
    scale_y_log10(
      breaks = trans_breaks("log10", function(x)
        10 ^ x),
      labels = trans_format("log10", math_format(10 ^ .x))
    ) +
    theme_bw()
}
