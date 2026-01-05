module github.com/myorg/api

go 1.21

require (
    github.com/gin-gonic/gin v1.9.0
    github.com/myorg/shared v0.0.0
)

replace github.com/myorg/shared => ../shared
