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
      `Ensemble Average` = rowMeans(x),
      `Ensemble Median` = apply(x, 1, median),
      `Expected Value` = EV ^ rounds
    ),
    cols = !1,
    names_to = "individual"
  )
}
