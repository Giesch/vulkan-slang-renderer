app [init!] { pf: platform "../platform/main.roc" }

import pf.InitConfig exposing [InitConfig]

init! : {} => InitConfig
init! = |{}| InitConfig.new({ window_title: "Basic Triangle from Roc" })
