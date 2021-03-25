module default {
    scalar type Hello extending str;
    scalar type World;
    type Type1 {
        property x -> str;
    }
    alias Bar := Type1 { x } { x };
};
alias default::Foo
