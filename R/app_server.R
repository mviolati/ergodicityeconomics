#' The application server-side
#'
#' @param input,output,session Internal parameters for {shiny}.
#' @import shiny
#' @noRd
app_server <- function(input, output, session)  {
  input_lose <- reactive({
    input$lose
  })

  input_win <- reactive({
    input$win
  })

  input_players <- reactive({
    input$players
  })

  input_rounds <- reactive({
    input$rounds
  })

  p <-
    reactive({
      coin_plot(coin_transform(coin_toss(input_lose(),
                                         input_win(),
                                         input_players(),
                                         input_rounds())))
      })
  output$myplot <- renderPlot({
    p()
  })
}


