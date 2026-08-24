package com.example.di;

import rustjvm.spring.context.Autowired;
import rustjvm.spring.context.Service;

@Service
public class GreetingService {

    @Autowired
    private PrefixService prefixService;

    public String greet(String name) {
        return prefixService.prefix() + "Hello, " + name + "!";
    }
}
