// inventory: e354_markov_switching_sums
// 7.1 refuses this file: `When all transitions probabilities are specified for a certain regime, they must sum to 1`
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
markov_switching(chain=1, number_of_regimes=2, duration=2.5, restrictions=[[1,2,0.4],[2,1,0.4],[1,1,0.4],[2,2,0.4]]);
