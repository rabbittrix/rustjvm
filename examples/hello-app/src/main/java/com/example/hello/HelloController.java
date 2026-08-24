package com.example.hello;

import rustjvm.spring.web.GetMapping;
import rustjvm.spring.web.RequestParam;
import rustjvm.spring.web.RestController;

@RestController
public class HelloController {

    @GetMapping("/hello")
    public String hello(@RequestParam String name) {
        return "Hello, " + name + "!";
    }

    @GetMapping("/ping")
    public String ping() {
        return "pong";
    }
}
