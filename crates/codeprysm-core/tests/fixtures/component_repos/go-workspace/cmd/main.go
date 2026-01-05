package main

import (
    "fmt"
    "myapp/shared"
    "myapp/api"
)

func main() {
    fmt.Println(shared.SharedFunc())
    fmt.Println(api.ApiHandler())
}
