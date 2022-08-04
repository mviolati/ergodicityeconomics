#' Description pending
#'
#' @param x
#'
#' @importFrom dplyr mutate
#' @importFrom tidyselect starts_with
#' @importFrom tidyr pivot_longer
#' @importFrom tibble rowid_to_column
#'
#' @export

coin_transform <- function(x) {
  x |>
    mutate(ensemble_avg = rowMeans(x),
           ensemble_median = apply(x, 1, median)) |>
    rowid_to_column("rounds") |>
    mutate(expected_value = EV ^ rounds) |>
    pivot_longer(cols = !starts_with("r"), names_to = "individual")
}
# Biggest bottleneck is pivot longer !!!!
