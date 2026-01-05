package api

import "myapp/shared"

func ApiHandler() string {
    return "api: " + shared.SharedFunc()
}
