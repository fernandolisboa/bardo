---
id: get-api-keys
title: Como obter cada chave de API
group: getting-started
---

# Como obter cada chave de API

Como obter a chave de cada provedor de [Configurações › Chaves de API](bardo:go/settings/keys), uma seção por provedor. No Bardo, o **Passo a passo** de cada cartão mostra os mesmos passos numa tela própria, com o campo do cartão no fim para colar a chave. Veja [Suas chaves de API](api-keys.md) para o que cada provedor faz.

Algumas coisas valem para todo provedor:

- **Copie a chave assim que ela aparecer.** A maioria dos provedores mostra uma chave nova uma única vez; se você perder, crie outra e troque a antiga no Bardo.
- **Guarde só para você.** Cole a chave apenas no Bardo; o Bardo guarda no Gerenciador de Credenciais do Windows e esconde a chave em todo o resto.
- **Teste.** Depois de salvar, **Testar chave** faz a chamada mais barata que precisa da chave e diz o que está errado, se houver algo.

<a id="claude"></a>
## Claude

1. Abra o [Claude Console](https://platform.claude.com/) e entre, ou crie uma conta.
2. Adicione créditos: abra [Settings › Billing](https://platform.claude.com/settings/billing) e escolha **Buy credits**. A API não responde sem saldo. Na mesma página dá para ligar a recarga automática (**Auto-reload**).
3. Abra [Settings › API keys](https://platform.claude.com/settings/keys) e escolha **Create key**. Dê um nome (por exemplo `Bardo`), escolha a validade e deixe **Linked account** como você mesmo.
4. Copie a chave. Ela começa com `sk-ant-` e aparece só desta vez.
5. Cole no cartão **Claude** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

<a id="elevenlabs"></a>
## ElevenLabs

1. Abra a [ElevenLabs](https://elevenlabs.io/app/sign-up) e entre, ou crie uma conta. A narração gasta os créditos do seu plano.
2. Abra [Developers › API Keys](https://elevenlabs.io/app/developers/api-keys) e escolha **Create key**. Dê um nome (por exemplo `Bardo`).
3. Deixe **Restrict Key** ligado, dê à chave acesso aos três recursos que o Bardo usa e deixe o resto em **No Access** (você também pode pôr um limite de créditos na chave):
   - **Text to Speech**: Access (a narração);
   - **Voices**: Read (as suas vozes, clones incluídos);
   - **Forced Alignment**: Access (o tempo de cada palavra para as legendas).
4. Escolha **Create key** e copie a chave. Ela aparece só desta vez.
5. Cole no cartão **ElevenLabs** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**. Uma chave sem um desses recursos falha com uma mensagem que diz qual falta.

<a id="gemini"></a>
## Gemini

1. Abra o [Google AI Studio](https://aistudio.google.com/) com a sua conta do Google e aceite os termos. Uma conta nova ganha um projeto do Google Cloud criado para ela; se você já usa o Google Cloud, traga um projeto em [Projects](https://aistudio.google.com/projects) com **Import projects**.
2. Configure o faturamento. Os modelos de imagem (Nano Banana) e de vídeo (Veo) do Google não têm nível gratuito, então o projeto precisa de uma conta de faturamento. Em [API Keys](https://aistudio.google.com/api-keys) ou [Projects](https://aistudio.google.com/projects), escolha **Set up billing** ao lado do projeto e siga os passos; uma conta pré-paga precisa de saldo, que você gerencia em [Billing](https://aistudio.google.com/billing). Quando o saldo pré-pago chega a zero, o Google recusa as chamadas até você recarregar.
3. Em [API Keys](https://aistudio.google.com/api-keys), escolha **Create API key** e o projeto.
4. Copie a chave. Dependendo de quando foi criada, ela começa com `AIza` ou com `AQ.`; copie a chave inteira.
5. Cole no cartão **Gemini** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

<a id="higgsfield"></a>
## Higgsfield

Você só precisa desta chave para clipes de vídeo dos modelos da Higgsfield.

1. Abra o [console da Higgsfield](https://open.higgsfield.ai/auth/sign-up) e crie uma conta, ou entre.
2. Adicione créditos em [Credits](https://open.higgsfield.ai/credits). Cada geração é paga com esse saldo; sem créditos, a Higgsfield recusa a chamada.
3. Abra [API keys](https://open.higgsfield.ai/api-keys) e crie uma chave. A Higgsfield dá duas partes para cada chave: um **ID da chave** (key ID) e uma **chave secreta** (secret).
4. Copie as duas. O Bardo recebe as duas num campo só, unidas por dois-pontos: `KEY_ID:KEY_SECRET` (o ID, um `:` e a chave secreta, sem espaços).
5. Cole no cartão **Higgsfield** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

<a id="typesafe"></a>
## TypeSafe (JEV)

1. Abra o [console da TypeSafe](https://console.typesafe.ai/) e entre, ou crie uma conta.
2. Abra [API keys](https://console.typesafe.ai/keys) e crie uma chave. Dê um nome (por exemplo `Bardo`).
3. Copie a chave.
4. Cole no cartão **TypeSafe (JEV)** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

A [documentação da TypeSafe](https://docs.typesafe.ai/introduction/quickstart) explica mais sobre as chaves e os modelos JEV.

<a id="youtube-data"></a>
## YouTube Data API

Esta é uma chave de API simples num projeto do Google Cloud, à parte do login no YouTube que o guia Conectar o YouTube configura. A API é gratuita dentro de uma cota diária de 10.000 unidades.

1. Abra o [console do Google Cloud](https://console.cloud.google.com/) e escolha um projeto, ou [crie um](https://console.cloud.google.com/projectcreate) (por exemplo `bardo`). O projeto do login do YouTube serve.
2. Ative a [YouTube Data API v3](https://console.cloud.google.com/apis/library/youtube.googleapis.com) com **Enable** (Ativar).
3. Abra [Credentials](https://console.cloud.google.com/apis/credentials) (Credenciais) e escolha **Create credentials › API key** (Criar credenciais › Chave de API).
4. Em **API restrictions**, escolha **Restrict key** e marque **YouTube Data API v3** (o Google pede uma restrição antes de criar a chave). Deixe **Authenticate API calls through a service account** desmarcado. Escolha **Create**.
5. Copie a chave de **API key created**. Ela começa com `AIza`.
6. Cole no cartão **YouTube Data API** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**. O teste usa 1 unidade da cota do dia.
