#'
#' Plotting with ggplot
#'
#' @param x the previous long tibble
#'
#' @importFrom ggplot2 ggplot aes geom_line scale_y_log10 theme_minimal theme labs
#' @importFrom scales trans_breaks trans_format math_format
#' @export

coin_plot <- function(x) {
  focus <- c("Ensemble Average", "Expected Value", "Ensemble Median")

  ggplot() +
    geom_line(
      subset(x, !individual %in% focus),
      mapping = aes(x = rounds, y = value, group = individual),
      size = 0.5,
      color = "grey",
      alpha = 0.2
    ) +
    geom_line(
      subset(x, individual %in% focus),
      mapping = aes(x = rounds, y = value, color = individual),
      size = 1
    ) +
    scale_y_log10(
      breaks = trans_breaks("log10", function(x)
        10 ^ x),
      labels = trans_format("log10", math_format(10 ^ .x))
    ) +
    theme_minimal(base_size = 13) +
    theme(legend.position = "bottom") +
    labs(x = "Rounds", y = "Value", color = "")

}
