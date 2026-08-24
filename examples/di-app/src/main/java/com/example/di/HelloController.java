package com.example.di;

import rustjvm.spring.context.Autowired;
import rustjvm.spring.web.GetMapping;
import rustjvm.spring.web.RequestParam;
import rustjvm.spring.web.RestController;

@RestController
public class HelloController {

    @Autowired
    private GreetingService greetingService;

    @GetMapping("/greet")
    public String greet(@RequestParam String name) {
        return greetingService.greet(name);
    }
}
