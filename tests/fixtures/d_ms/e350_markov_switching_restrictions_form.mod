// inventory: e350_markov_switching_restrictions_form
// 7.1 refuses this file: `restrictions in the subsample statement must be specified in the form [current_period_regime, next_period_regime, transition_probability]`
var R Pie Y;
varexo eps;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
model;
R = beta*R(-1) + eps;
Pie = alpha*R(-1) + eps;
Y = beta*Pie(-1) + eps;
end;
initval;
R = 0;
Pie = 0;
Y = 0;
end;
shocks;
var eps; stderr 0.1;
end;
varobs Y Pie R;
markov_switching(chain=1, number_of_regimes=2, duration=2.5, restrictions=[[1,2]]);
