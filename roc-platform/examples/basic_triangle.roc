app [init!] { pf: platform "../platform/main.roc" }

init! : {} => { window_title : Str }
init! = |{}| { window_title: "Basic Triangle from Roc" }
