#'
#' Adding Median, Mean and EV -> Transforming to long
#'
#' !!!! Biggest bottleneck is pivot longer !!!!
#' !!!! Consider keeping wide !!!!
#'
#' @param x
#'
#' @importFrom dplyr mutate
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
    pivot_longer(cols = !1, names_to = "individual")
}
