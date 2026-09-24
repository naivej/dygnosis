// inventory: p_pac_e001_component_string
var x;
parameters beta;
beta = .9;
model;
  x = x(-1);
end;
pac_model(model_name=p, discount=beta);

pac_target_info(p);
  target x;
  component 'x';
  kind ll;
end;
