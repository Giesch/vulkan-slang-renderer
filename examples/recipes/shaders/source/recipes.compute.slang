#language slang 2026

module recipes_compute;

import mltrs;
import shared;

ParameterBlock<IngredientParams> params;

struct IngredientParams {
    int4x4 weights;
    int4 calories;
    mltrs::Addr<Solution> solution;
}

[numthreads(10, 10, 10)]
[shader("compute")]
void computeMain(uint3 thread : SV_DispatchThreadID) {
    let sum = thread.x + thread.y + thread.z;
    let recipeInvalid = sum > 100;
    if (recipeInvalid) {
        return;
    }

    let lastIngredient = 100 - sum;
    let recipe = int4(thread, lastIngredient);

    let weighted = mul(recipe, params.weights);
    let clamped = max(weighted, int4(0));
    let product: uint = clamped.x * clamped.y * clamped.z * clamped.w;

    // Part 1
    // InterlockedMax(params.solution.maximumScore, product);

    // Part 2
    let calories = params.calories * recipe;
    let caloriesTotal = calories.x + calories.y + calories.z + calories.w;
    if (caloriesTotal == 500) {
        InterlockedMax(params.solution.maximumScore, product);
    }
}
