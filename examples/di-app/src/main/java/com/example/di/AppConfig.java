package com.example.di;

import rustjvm.spring.context.Bean;
import rustjvm.spring.context.Configuration;

@Configuration
public class AppConfig {

    @Bean
    public PrefixService prefixService() {
        return new PrefixService();
    }
}
