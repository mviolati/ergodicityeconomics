#' Description pending
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
#' gamble rounds or repetitions
#'
#' @param players
#' number of players
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
                      ))), .name_repair = "unique")
}
# Players and round both greater than 10^5 gives error
# Error: cannot allocate vector of size 74.5 Gb
# Limit players to <= 10^4 and rounds <= 10^4
# Replicate Time spent on sapply -> lapply
