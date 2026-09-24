// inventory: p_pac_e261_deterministic_duplicate
var x;
parameters beta;
beta = .9;
model;
  x = x(-1);
end;

deterministic_trends;
  x (beta);
  x (beta+1);
end;
