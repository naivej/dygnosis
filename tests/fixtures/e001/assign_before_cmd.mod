// inventory: e001_assign_before_cmd
// Original AR(1) plus Euler. Missing ';' before a catalog command must still be E001.
var y c;
varexo e;
parameters rho betta alpha sigmae;
rho = 0.90;
betta = 0.99;
alpha = 0.33;
sigmae = 0.01;

model;
y = rho * y(-1) + e;
c = betta * c(+1) * alpha;
end;

shocks;
var e; stderr sigmae;
end;

initval;
y = 0;
c = 1;
end;

steady;

scale = 1
method_of_moments(
    mom_method = irf_matching,
    order = 1,
    mode_compute = 5
);
