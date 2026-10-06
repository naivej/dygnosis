// inventory: e001_macro_decl_money0
@#define money_growth_rule = 0
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
alpha = 0.33;
model;
pi = beta * pi(+1) + alpha * x + e + u;
x = 0;
end;
