use crate::data::TextClassificationBatch;
use burn::{
    config::Config,
    module::{Module, Param},
    nn::{
        loss::CrossEntropyLoss,
        transformer::{TransformerEncoder, TransformerEncoderConfig, TransformerEncoderInput},
        Embedding, EmbeddingConfig, Linear, LinearConfig,
    },
    tensor::backend::{ADBackend, Backend},
    tensor::Tensor,
    train::{ClassificationOutput, TrainOutput, TrainStep, ValidStep},
};

#[derive(Config)]
pub struct TextClassificationModelConfig {
    transformer: TransformerEncoderConfig,
    n_classes: usize,
    vocab_size: usize,
    max_seq_length: usize,
}

#[derive(Module, Debug)]
pub struct TextClassificationModel<B: Backend> {
    transformer: Param<TransformerEncoder<B>>,
    embedding_token: Param<Embedding<B>>,
    embedding_pos: Param<Embedding<B>>,
    output: Param<Linear<B>>,
    n_classes: usize,
    max_seq_length: usize,
}

impl<B: Backend> TextClassificationModel<B> {
    pub fn new(config: &TextClassificationModelConfig) -> Self {
        let config_embedding_token =
            EmbeddingConfig::new(config.vocab_size, config.transformer.d_model);
        let config_embedding_pos =
            EmbeddingConfig::new(config.max_seq_length, config.transformer.d_model);
        let config_output = LinearConfig::new(config.transformer.d_model, config.n_classes);

        let transformer = TransformerEncoder::new(&config.transformer);
        let embedding_token = Embedding::new(&config_embedding_token);
        let embedding_pos = Embedding::new(&config_embedding_pos);
        let output = Linear::new(&config_output);

        Self {
            transformer: Param::new(transformer),
            embedding_token: Param::new(embedding_token),
            embedding_pos: Param::new(embedding_pos),
            output: Param::new(output),
            n_classes: config.n_classes,
            max_seq_length: config.max_seq_length,
        }
    }

    pub fn forward(&self, item: TextClassificationBatch<B>) -> ClassificationOutput<B> {
        let [batch_size, seq_length] = item.tokens.dims();
        let device = self.embedding_token.devices()[0];

        let tokens = item.tokens.to_device(device).detach();
        let labels = item.labels.to_device(device).detach();
        let mask_pad = item.mask_pad.to_device(device);
        std::mem::drop(item);

        let index_positions = Tensor::<B, 1>::arange_device(0..seq_length, device)
            .reshape([1, seq_length])
            .repeat(0, batch_size);
        let embedding_positions = self.embedding_pos.forward(index_positions.detach());
        let embedding_tokens = self.embedding_token.forward(tokens.detach());
        let embedding = (embedding_positions + embedding_tokens) / 2;

        let encoded = self
            .transformer
            .forward(TransformerEncoderInput::new(embedding).mask_pad(mask_pad));
        let output = self.output.forward(encoded);

        let output_classification = output
            .index([0..batch_size, 0..1])
            .reshape([batch_size, self.n_classes]);

        let loss = CrossEntropyLoss::new(self.n_classes, None);
        let loss = loss.forward(&output_classification, &labels);

        ClassificationOutput {
            loss,
            output: output_classification,
            targets: labels,
        }
    }
}

impl<B: ADBackend> TrainStep<B, TextClassificationBatch<B>, ClassificationOutput<B>>
    for TextClassificationModel<B>
{
    fn step(&self, item: TextClassificationBatch<B>) -> TrainOutput<B, ClassificationOutput<B>> {
        let item = self.forward(item);
        let grads = item.loss.backward();

        TrainOutput::new(self, grads, item)
    }
}

impl<B: Backend> ValidStep<TextClassificationBatch<B>, ClassificationOutput<B>>
    for TextClassificationModel<B>
{
    fn step(&self, item: TextClassificationBatch<B>) -> ClassificationOutput<B> {
        self.forward(item)
    }
}
