#' The application User-Interface
#'
#' @param request Internal parameter for `{shiny}`.
#'     DO NOT REMOVE.
#' @import shiny
#' @noRd
app_ui <- function(request) {
  tagList(# Leave this function for adding external resources
    golem_add_external_resources(),
    # Your application UI logic
    fluidPage(
      titlePanel("Simple Gamble"),
      fluidRow(
        column(
          12,
          markdown(
            'First objective was to replicate Fig. 2 of this [paper](https://rdcu.be/cS2t3).
               For non-technical may be hard to understand what is happening without modeling.
                        Emanuel Derman\'s [tweet](https://twitter.com/EmanuelDerman/status/1532473709239455745)
                        stresses out that *"a 200% - 50% equal-probability gamble has an expected payoff of 125% if played once,
                        but if you keep playing the eventual return is 50% x 200% = 100% in long run, no gain."*'
          )
        ),
        column(
          4,
          sliderInput("lose", "Losing Percentage", 0, 1, 0.6, 0.1),
          sliderInput("win", "Winning  Percentage", 1, 2, 1.5, 0.1),
          sliderInput("players", "Number of Players", 0, 10 ^ 3, 150, 10),
          sliderInput("rounds", "Number of rounds of the game", 0, 10 ^
                        3, 10 ^ 3, 10)
        ),
        column(8, plotOutput("myplot"))
      )
    ))
}



#' Add external Resources to the Application
#'
#' This function is internally used to add external
#' resources inside the Shiny application.
#'
#' @import shiny
#' @importFrom golem add_resource_path activate_js favicon bundle_resources
#' @noRd
golem_add_external_resources <- function() {
  add_resource_path(
    "www",
    app_sys("app/www")
  )

  tags$head(
    favicon(),
    bundle_resources(
      path = app_sys("app/www"),
      app_title = "eecon.app"
    )
    # Add here other external resources
    # for example, you can add shinyalert::useShinyalert()
  )
}
