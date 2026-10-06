// inventory: e001_macro_decl_money1
@#define money_growth_rule = 1
var pi
@#if money_growth_rule == 0
x
@#else
m
@#endif
;
varexo e
@#if money_growth_rule == 0
u
@#else
v
@#endif
;
parameters beta
@#if money_growth_rule == 0
alpha
@#else
gamma
@#endif
;
beta = 0.99;
gamma = 0.33;
model;
pi = beta * pi(+1) + gamma * m + e + v;
m = 0;
end;
