package com.example.petstore;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class PetController {

    @Autowired
    private PetService petService;

    @GetMapping("/pet")
    public String pet(@RequestParam String id) {
        return petService.describe(id);
    }
}
