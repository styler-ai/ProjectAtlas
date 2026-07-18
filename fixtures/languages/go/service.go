package atlas

import "fmt"

type Runner struct {}

func (r Runner) Run() {
    helper()
}

func helper() {
    fmt.Println("ok")
}
