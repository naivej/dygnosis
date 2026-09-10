// inventory: w122_auto
var y c;
varexo e;
parameters rho betta optimal_policy_discount_factor;
rho = 0.9;
betta = 0.99;
optimal_policy_discount_factor = Inf;

model;
y = rho * y(-1) + e;
c = betta * c(+1);
y = rho * y(-1) + e + 0 * optimal_policy_discount_factor;
end;

shocks;
var e; stderr 0.01;
end;

steady_state_model;
y = 0;
c = 0;
end;
steady;
