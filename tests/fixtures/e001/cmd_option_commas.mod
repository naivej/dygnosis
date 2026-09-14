// inventory: e001_cmd_option_commas, auto_fix_cmd_option_commas
// Original AR(1) plus a consumption Euler. Locks command option-list commas.
var y c;
varexo e;
parameters rho betta sigmae;
rho = 0.90;
betta = 0.99;
sigmae = 0.01;

model;
y = rho * y(-1) + e;
c = betta * c(+1);
end;

shocks;
var e; stderr sigmae;
end;

initval;
y = 0;
c = 1;
end;

steady;

method_of_moments(
    mom_method = irf_matching,
    order = 1,
    mode_compute = 5
);

forecast(
    periods = 10,
    conf_sig = 0.9
);

stoch_simul(
    order = 1,
    irf = 20,
    nograph
);
