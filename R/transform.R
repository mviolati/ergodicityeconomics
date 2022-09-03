#'
#' Adding Median, Mean and EV -> Transforming to long
#'
#' @param x the tibble of random numbers
#'
#' @importFrom dplyr mutate
#' @importFrom tidyr pivot_longer
#' @importFrom tibble rowid_to_column
#' @importFrom stats median
#'
#' @export

coin_transform <- function(x) {
  pivot_longer(
    mutate(
      rowid_to_column(x, "rounds"),
      ensemble_avg = rowMeans(x),
      ensemble_median = apply(x, 1, median),
      expected_value = EV ^ rounds
    ),
    cols = !1,
    names_to = "individual"
  )
}
