// inventory: e302_planner_discount_policy
var y;
varexo e;
parameters rho optimal_policy_discount_factor;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
ramsey_policy(planner_discount=0.99);
