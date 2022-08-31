#'
#' Creating random sample
#'
#' Limit players to <= 10^4
#' Replicate Time spent on sapply -> lapply
#' Long df is faster than wider
#'
#' @param win
#' percentage of wealth increase if you win
#' you win 1.5 of your current wealth
#'
#' @param lose
#' percentage of wealth decrease if you lose
#' you lose to 0.6 of your current wealth
#'
#' @param rounds
#' gamble rounds or repetitions - max 10^3
#'
#' @param players
#' number of players - max 10^5
#'
#' @importFrom tibble as_tibble
#'
#' @export

coin_toss <- function(lose, win, players, rounds) {
  assign("EV", ((lose * 0.5) + (win * 0.5)), envir = .GlobalEnv)
  as_tibble(replicate(players,
                      cumprod(sample(
                        c(lose, win),
                        size = rounds,
                        replace = TRUE
                      ))))
}
